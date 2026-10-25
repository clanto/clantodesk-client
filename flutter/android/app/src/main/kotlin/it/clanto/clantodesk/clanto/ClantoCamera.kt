// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

package it.clanto.clantodesk.clanto

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.ImageFormat
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CaptureRequest
import android.media.ImageReader
import android.os.Handler
import android.os.HandlerThread
import android.util.Log
import android.util.Range
import android.util.Size
import java.nio.ByteBuffer

/**
 * La webcam del telefono come webcam del tecnico (src/clanto/forwarding/
 * camera_capture_android.rs). Camera2, frame YUV_420_888 fino a 1280x720
 * passati a Rust; quando Rust risponde `false` (flusso finito) la camera si chiude.
 */
object ClantoCamera {
    private const val TAG = "ClantoCamera"
    private const val MAX_WIDTH = 1280
    private const val MAX_HEIGHT = 720

    @JvmStatic
    external fun pushFrame(
        y: ByteBuffer, u: ByteBuffer, v: ByteBuffer,
        yStride: Int, uvStride: Int, uvPixelStride: Int,
        width: Int, height: Int, rotation: Int,
    ): Boolean

    private var thread: HandlerThread? = null
    private var device: CameraDevice? = null
    private var session: CameraCaptureSession? = null
    private var reader: ImageReader? = null

    /** La dimensione più grande entro 1280x720, o la più piccola disponibile. */
    fun chooseSize(sizes: List<Size>): Size? =
        sizes.filter { it.width <= MAX_WIDTH && it.height <= MAX_HEIGHT }
            .maxByOrNull { it.width * it.height }
            ?: sizes.minByOrNull { it.width * it.height }

    /** `front` = camera anteriore. Restituisce un errore o null. Il permesso CAMERA va già concesso. */
    @SuppressLint("MissingPermission")
    @Synchronized
    fun start(context: Context, front: Boolean): String? {
        stop()
        val manager = context.getSystemService(Context.CAMERA_SERVICE) as CameraManager
        val facing = if (front) CameraCharacteristics.LENS_FACING_FRONT else CameraCharacteristics.LENS_FACING_BACK
        val id = manager.cameraIdList.firstOrNull {
            manager.getCameraCharacteristics(it).get(CameraCharacteristics.LENS_FACING) == facing
        } ?: return "No camera found"
        val chars = manager.getCameraCharacteristics(id)
        val map = chars.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP) ?: return "No camera found"
        val size = chooseSize(map.getOutputSizes(ImageFormat.YUV_420_888).toList()) ?: return "No camera found"
        // Telefono in verticale: la rotazione del sensore raddrizza l'immagine.
        val rotation = chars.get(CameraCharacteristics.SENSOR_ORIENTATION) ?: 0
        val fps = chars.get(CameraCharacteristics.CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES)
            ?.filter { it.upper in 15..30 }?.maxByOrNull { it.upper }

        val t = HandlerThread("clanto-camera").also { it.start() }
        val handler = Handler(t.looper)
        val r = ImageReader.newInstance(size.width, size.height, ImageFormat.YUV_420_888, 2)
        r.setOnImageAvailableListener({ ir ->
            val image = ir.acquireLatestImage() ?: return@setOnImageAvailableListener
            val keep = try {
                val p = image.planes
                pushFrame(
                    p[0].buffer, p[1].buffer, p[2].buffer,
                    p[0].rowStride, p[1].rowStride, p[1].pixelStride,
                    image.width, image.height, rotation,
                )
            } catch (e: Throwable) {
                Log.w(TAG, "frame: $e")
                true
            } finally {
                image.close()
            }
            if (!keep) handler.post { stop() }
        }, handler)
        thread = t
        reader = r

        manager.openCamera(id, object : CameraDevice.StateCallback() {
            override fun onOpened(camera: CameraDevice) {
                device = camera
                @Suppress("DEPRECATION")
                camera.createCaptureSession(listOf(r.surface), object : CameraCaptureSession.StateCallback() {
                    override fun onConfigured(s: CameraCaptureSession) {
                        session = s
                        val request = camera.createCaptureRequest(CameraDevice.TEMPLATE_RECORD).apply {
                            addTarget(r.surface)
                            fps?.let { set(CaptureRequest.CONTROL_AE_TARGET_FPS_RANGE, Range(it.lower, it.upper)) }
                        }
                        try {
                            s.setRepeatingRequest(request.build(), null, handler)
                        } catch (e: Exception) {
                            Log.w(TAG, "repeating request: $e")
                            stop()
                        }
                    }

                    override fun onConfigureFailed(s: CameraCaptureSession) {
                        Log.w(TAG, "session configuration failed")
                        stop()
                    }
                }, handler)
            }

            override fun onDisconnected(camera: CameraDevice) = stop()

            override fun onError(camera: CameraDevice, error: Int) {
                Log.w(TAG, "camera error $error")
                stop()
            }
        }, handler)
        return null
    }

    @Synchronized
    fun stop() {
        try {
            session?.close()
            device?.close()
            reader?.close()
        } catch (e: Exception) {
            Log.w(TAG, "stop: $e")
        }
        session = null
        device = null
        reader = null
        thread?.quitSafely()
        thread = null
    }
}
