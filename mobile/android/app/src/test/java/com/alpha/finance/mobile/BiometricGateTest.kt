package com.alpha.finance.mobile

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import javax.crypto.KeyGenerator

/**
 * 生物识别门与隐私保护单测（JVM 级，无需设备/.so，L512
 * docs/mobile-privacy.md §5）：隐私设置持久化与默认值、门状态机全部
 * 迁移规则、AES-GCM 加密存储语义（往返/防篡改/IV 唯一性/封顶）。
 * Keystore/Prompt 设备路径不在此覆盖（无 Android 框架类）。
 */
class BiometricGateTest {

    // ---------- PrivacySettingsStore ----------

    @Test
    fun `默认值：生物识别关 opt-in，退后台锁与防截屏默认开`() {
        val store = PrivacySettingsStore(InMemoryKeyValueStore())
        assertEquals(PrivacySettings(), store.load())
        assertEquals(false, store.load().biometricEnabled)
        assertEquals(true, store.load().lockOnBackground)
        assertEquals(true, store.load().screenshotShield)
    }

    @Test
    fun `设置往返与损坏回退默认`() {
        val kv = InMemoryKeyValueStore()
        val store = PrivacySettingsStore(kv)
        store.save(PrivacySettings(biometricEnabled = true, lockOnBackground = false))
        assertEquals(
            PrivacySettings(biometricEnabled = true, lockOnBackground = false),
            store.load(),
        )
        // 损坏 JSON → fail-safe 回默认（锁屏开）
        kv.put(PrivacySettingsStore.SETTINGS_KEY, "{not json")
        assertEquals(PrivacySettings(), store.load())
    }

    // ---------- GateStateMachine ----------

    private fun lockedGate(lockOnBackground: Boolean = true) = GateStateMachine(
        PrivacySettings(biometricEnabled = true, lockOnBackground = lockOnBackground),
    )

    @Test
    fun `初始会话：开生物识别门且退后台锁 → 冷启动即锁`() {
        assertEquals(LockState.Locked, lockedGate().state)
        // 关掉「退后台锁」→ 门只在显式锁定语义时锁（此处不冷启动锁）
        assertEquals(LockState.Unlocked, lockedGate(lockOnBackground = false).state)
    }

    @Test
    fun `退后台重锁并清失败计数`() {
        val gate = lockedGate()
        gate.onAuthFailure()
        gate.onAuthFailure()
        assertEquals(2, gate.failedAttempts)
        gate.onBackground()
        assertEquals(LockState.Locked, gate.state)
        assertEquals(0, gate.failedAttempts)
        // lockOnBackground=false 时退后台不锁、不清计数
        val loose = lockedGate(lockOnBackground = false)
        loose.onAuthFailure()
        loose.onBackground()
        assertEquals(LockState.Unlocked, loose.state)
        assertEquals(1, loose.failedAttempts)
    }

    @Test
    fun `认证成功解锁；失败留在锁态并累计`() {
        val gate = lockedGate()
        gate.onAuthFailure()
        assertEquals(LockState.Locked, gate.state)
        assertEquals(1, gate.failedAttempts)
        gate.onAuthSuccess()
        assertEquals(LockState.Unlocked, gate.state)
        assertEquals(0, gate.failedAttempts)
    }

    @Test
    fun `关闭生物识别立即解锁`() {
        val gate = lockedGate()
        assertEquals(LockState.Locked, gate.state)
        gate.updateSettings(PrivacySettings(biometricEnabled = false))
        assertEquals(LockState.Unlocked, gate.state)
        assertEquals(0, gate.failedAttempts)
        // 开启「退后台即锁」不突袭锁定当前会话
        gate.updateSettings(PrivacySettings(biometricEnabled = true, lockOnBackground = true))
        assertEquals(LockState.Unlocked, gate.state)
    }

    // ---------- EncryptedKeyValueStore ----------

    private fun aesKey() = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()

    @Test
    fun `加密往返与未写入键`() {
        val store = EncryptedKeyValueStore(InMemoryKeyValueStore(), aesKey())
        store.put("quote:600519", """{"symbol":"600519","price":90.5}""")
        assertEquals("""{"symbol":"600519","price":90.5}""", store.get("quote:600519"))
        assertNull(store.get("absent"))
    }

    @Test
    fun `密文不泄明文且同明文两次写入密文不同（随机 IV）`() {
        val inner = InMemoryKeyValueStore()
        val store = EncryptedKeyValueStore(inner, aesKey())
        store.put("k", "plaintext-secret")
        val blob1 = inner.get("k")!!
        assertTrue("落盘值不含明文", !blob1.contains("plaintext-secret"))
        store.put("k", "plaintext-secret")
        val blob2 = inner.get("k")!!
        assertNotEquals("GCM 每写一次新 IV，密文必不同", blob1, blob2)
        assertEquals("plaintext-secret", store.get("k"))
    }

    @Test
    fun `密文被篡改或换钥解密 → get 返回 null 不抛错`() {
        val inner = InMemoryKeyValueStore()
        val store = EncryptedKeyValueStore(inner, aesKey())
        store.put("k", "secret")
        // 位翻转篡改（IV 首字节）→ GCM 认证标签不过
        inner.put("k", buildString {
            val c = inner.get("k")!!
            val flipped = ('a'.code + ((c[0].code - 'a'.code + 1) % 26))
            append(flipped)
            append(c.substring(1))
        })
        assertNull("篡改密文不得解出内容", store.get("k"))
        // 换钥 → 同样解不出
        val wrongKey = EncryptedKeyValueStore(inner, aesKey())
        store.put("k2", "secret")
        assertNull(wrongKey.get("k2"))
        // 垃圾 blob → null
        inner.put("k3", "not-base64!!!")
        assertNull(store.get("k3"))
    }

    @Test
    fun `delete 与 keys 透传内层`() {
        val inner = InMemoryKeyValueStore()
        val store = EncryptedKeyValueStore(inner, aesKey())
        store.put("a", "1")
        store.put("b", "2")
        assertEquals(listOf("a", "b"), store.keys())
        store.delete("a")
        assertNull(store.get("a"))
        assertEquals(listOf("b"), store.keys())
    }
}
