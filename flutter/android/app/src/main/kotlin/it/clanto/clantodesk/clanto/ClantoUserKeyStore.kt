// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

package it.clanto.clantodesk.clanto

import android.annotation.TargetApi
import android.content.Context
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.io.File
import java.io.FileOutputStream
import java.security.GeneralSecurityException
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

object ClantoUserKeyStore {
    private const val KEY_ALIAS = "it.clanto.ClantoDesk.user-key"
    private const val FILE_NAME = "clantodesk-user-key.v1"
    private const val KEY_LENGTH = 32
    private const val FORMAT_VERSION: Byte = 1
    private const val GCM_TAG_BITS = 128

    @JvmStatic
    @Synchronized
    fun hasStoredUserKey(context: Context): Boolean =
        File(context.applicationContext.filesDir, FILE_NAME).exists()

    @JvmStatic
    @Synchronized
    fun readUserKey(context: Context): ByteArray? {
        val file = File(context.applicationContext.filesDir, FILE_NAME)
        if (!file.exists()) {
            return null
        }
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) {
            throw GeneralSecurityException("Android Keystore AES-GCM requires API 23")
        }
        val stored = file.readBytes()
        if (stored.size < 3 || stored[0] != FORMAT_VERSION) {
            throw GeneralSecurityException("invalid protected user key format")
        }
        val ivLength = stored[1].toInt() and 0xff
        if (ivLength == 0 || stored.size <= 2 + ivLength) {
            throw GeneralSecurityException("invalid protected user key IV")
        }
        val iv = stored.copyOfRange(2, 2 + ivLength)
        val ciphertext = stored.copyOfRange(2 + ivLength, stored.size)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, existingKey(), GCMParameterSpec(GCM_TAG_BITS, iv))
        return cipher.doFinal(ciphertext).also {
            if (it.size != KEY_LENGTH) {
                throw GeneralSecurityException("protected user key has invalid length")
            }
        }
    }

    @JvmStatic
    @Synchronized
    fun storeUserKey(context: Context, key: ByteArray): Boolean {
        if (key.size != KEY_LENGTH || Build.VERSION.SDK_INT < Build.VERSION_CODES.M) {
            return false
        }
        val appContext = context.applicationContext
        val destination = File(appContext.filesDir, FILE_NAME)
        if (destination.exists()) {
            return readUserKey(appContext)?.contentEquals(key) == true
        }

        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, existingOrCreateKey())
        val ciphertext = cipher.doFinal(key)
        val iv = cipher.iv
        if (iv.isEmpty() || iv.size > 255) {
            throw GeneralSecurityException("invalid Android Keystore IV")
        }
        val stored = byteArrayOf(FORMAT_VERSION, iv.size.toByte()) + iv + ciphertext
        val temporary = File.createTempFile(".$FILE_NAME.", ".tmp", appContext.filesDir)
        try {
            FileOutputStream(temporary).use {
                it.write(stored)
                it.fd.sync()
            }
            if (!temporary.renameTo(destination)) {
                if (!destination.exists()) {
                    throw GeneralSecurityException("failed to install protected user key")
                }
                return readUserKey(appContext)?.contentEquals(key) == true
            }
        } finally {
            temporary.delete()
        }
        return readUserKey(appContext)?.contentEquals(key) == true
    }

    private fun existingKey(): SecretKey {
        val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        return keyStore.getKey(KEY_ALIAS, null) as? SecretKey
            ?: throw GeneralSecurityException("Android Keystore user key is missing")
    }

    @TargetApi(Build.VERSION_CODES.M)
    private fun existingOrCreateKey(): SecretKey {
        val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (keyStore.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(
            KeyGenParameterSpec.Builder(
                KEY_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .setRandomizedEncryptionRequired(true)
                .build(),
        )
        return generator.generateKey()
    }
}
