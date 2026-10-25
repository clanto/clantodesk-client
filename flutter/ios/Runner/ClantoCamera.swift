// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

import AVFoundation
import Flutter
import UIKit

/// La fotocamera dell'iPhone come webcam del tecnico (forwarding.dart, canale
/// "it.clanto.clantodesk/camera"). Frame NV12 1280x720 diritti in verticale,
/// passati a Rust con `clanto_camera_push_nv12`; quando Rust risponde false (flusso
/// finito) la sessione si ferma. iOS concede la camera solo con l'app in primo piano.
final class ClantoCamera: NSObject, AVCaptureVideoDataOutputSampleBufferDelegate {
    static let shared = ClantoCamera()
    private let queue = DispatchQueue(label: "it.clanto.clantodesk.camera")
    private var session: AVCaptureSession?

    static func register(messenger: FlutterBinaryMessenger) {
        let channel = FlutterMethodChannel(name: "it.clanto.clantodesk/camera", binaryMessenger: messenger)
        channel.setMethodCallHandler { call, result in
            switch call.method {
            case "start":
                shared.start(front: call.arguments as? Bool ?? false) { result($0) }
            case "stop":
                shared.stop()
                result(nil)
            default:
                result(FlutterMethodNotImplemented)
            }
        }
    }

    /// `done(nil)` se la camera è partita, altrimenti il motivo.
    func start(front: Bool, done: @escaping (String?) -> Void) {
        AVCaptureDevice.requestAccess(for: .video) { granted in
            guard granted else {
                DispatchQueue.main.async { done("Camera access denied") }
                return
            }
            self.queue.async {
                let error = self.configure(front: front)
                DispatchQueue.main.async { done(error) }
            }
        }
    }

    private func configure(front: Bool) -> String? {
        stopOnQueue()
        guard let device = AVCaptureDevice.default(.builtInWideAngleCamera, for: .video, position: front ? .front : .back),
              let input = try? AVCaptureDeviceInput(device: device) else {
            return "No camera found"
        }
        let session = AVCaptureSession()
        session.beginConfiguration()
        session.sessionPreset = session.canSetSessionPreset(.hd1280x720) ? .hd1280x720 : .vga640x480
        let output = AVCaptureVideoDataOutput()
        output.videoSettings = [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange]
        output.alwaysDiscardsLateVideoFrames = true
        output.setSampleBufferDelegate(self, queue: queue)
        guard session.canAddInput(input), session.canAddOutput(output) else {
            session.commitConfiguration()
            return "No camera found"
        }
        session.addInput(input)
        session.addOutput(output)
        if let connection = output.connection(with: .video) {
            // Telefono in verticale: frame già diritti, niente specchio.
            if #available(iOS 17.0, *) {
                if connection.isVideoRotationAngleSupported(90) {
                    connection.videoRotationAngle = 90
                }
            } else if connection.isVideoOrientationSupported {
                connection.videoOrientation = .portrait
            }
            if connection.isVideoMirroringSupported {
                connection.automaticallyAdjustsVideoMirroring = false
                connection.isVideoMirrored = false
            }
        }
        if (try? device.lockForConfiguration()) != nil {
            device.activeVideoMinFrameDuration = CMTime(value: 1, timescale: 30)
            device.unlockForConfiguration()
        }
        session.commitConfiguration()
        session.startRunning()
        self.session = session
        return nil
    }

    func stop() {
        queue.async { self.stopOnQueue() }
    }

    private func stopOnQueue() {
        session?.stopRunning()
        session = nil
    }

    func captureOutput(_ output: AVCaptureOutput, didOutput sampleBuffer: CMSampleBuffer, from connection: AVCaptureConnection) {
        guard let image = CMSampleBufferGetImageBuffer(sampleBuffer),
              CVPixelBufferGetPlaneCount(image) == 2 else { return }
        CVPixelBufferLockBaseAddress(image, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(image, .readOnly) }
        guard let y = CVPixelBufferGetBaseAddressOfPlane(image, 0),
              let uv = CVPixelBufferGetBaseAddressOfPlane(image, 1) else { return }
        let yStride = CVPixelBufferGetBytesPerRowOfPlane(image, 0)
        let uvStride = CVPixelBufferGetBytesPerRowOfPlane(image, 1)
        let keep = clanto_camera_push_nv12(
            y.assumingMemoryBound(to: UInt8.self), yStride * CVPixelBufferGetHeightOfPlane(image, 0), Int32(yStride),
            uv.assumingMemoryBound(to: UInt8.self), uvStride * CVPixelBufferGetHeightOfPlane(image, 1), Int32(uvStride),
            Int32(CVPixelBufferGetWidth(image)), Int32(CVPixelBufferGetHeight(image)))
        if !keep {
            stopOnQueue()
        }
    }
}
