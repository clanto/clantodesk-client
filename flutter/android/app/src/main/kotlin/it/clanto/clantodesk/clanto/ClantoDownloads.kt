// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

package it.clanto.clantodesk.clanto

import android.content.ContentValues
import android.content.Context
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.media.MediaScannerConnection
import android.webkit.MimeTypeMap
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream

/** Pubblica un file privato dell'app nella raccolta Download di Android. */
object ClantoDownloads {
    fun publish(context: Context, sourcePath: String, displayName: String): String {
        val source = File(sourcePath)
        require(source.isFile) { "File sorgente non trovato" }
        val safeName = displayName.substringAfterLast('/').substringAfterLast('\\')
        require(safeName.isNotBlank()) { "Nome file non valido" }

        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            publishWithMediaStore(context, source, safeName)
        } else {
            publishLegacy(context, source, safeName)
        }
    }

    private fun publishWithMediaStore(
        context: Context,
        source: File,
        displayName: String
    ): String {
        val extension = displayName.substringAfterLast('.', "")
        val mimeType = MimeTypeMap.getSingleton()
            .getMimeTypeFromExtension(extension.lowercase()) ?: "application/octet-stream"
        val values = ContentValues().apply {
            put(MediaStore.Downloads.DISPLAY_NAME, displayName)
            put(MediaStore.Downloads.MIME_TYPE, mimeType)
            put(MediaStore.Downloads.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS)
            put(MediaStore.Downloads.IS_PENDING, 1)
        }
        val resolver = context.contentResolver
        val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
            ?: error("Impossibile creare il file in Download")
        try {
            resolver.openOutputStream(uri, "w").use { output ->
                requireNotNull(output) { "Impossibile aprire il file in Download" }
                FileInputStream(source).use { input -> input.copyTo(output) }
            }
            values.clear()
            values.put(MediaStore.Downloads.IS_PENDING, 0)
            resolver.update(uri, values, null, null)
            return uri.toString()
        } catch (e: Exception) {
            resolver.delete(uri, null, null)
            throw e
        }
    }

    @Suppress("DEPRECATION")
    private fun publishLegacy(context: Context, source: File, displayName: String): String {
        val downloads = Environment.getExternalStoragePublicDirectory(
            Environment.DIRECTORY_DOWNLOADS
        )
        check(downloads.exists() || downloads.mkdirs()) { "Cartella Download non disponibile" }
        var destination = File(downloads, displayName)
        var suffix = 1
        val base = displayName.substringBeforeLast('.', displayName)
        val extension = displayName.substringAfterLast('.', "")
        while (destination.exists()) {
            val candidate = if (extension.isEmpty()) {
                "$base ($suffix)"
            } else {
                "$base ($suffix).$extension"
            }
            destination = File(downloads, candidate)
            suffix++
        }
        FileInputStream(source).use { input ->
            FileOutputStream(destination).use { output -> input.copyTo(output) }
        }
        MediaScannerConnection.scanFile(
            context,
            arrayOf(destination.absolutePath),
            null,
            null
        )
        return destination.absolutePath
    }
}
