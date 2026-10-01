package com.alpha.finance.mobile

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import androidx.biometric.BiometricManager
import androidx.biometric.BiometricPrompt
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import java.security.KeyStore
import java.util.Base64
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * 生物识别门与隐私保护（L512，docs/mobile-privacy.md）。
 *
 * 两层分离（职责不混用）：
 * - **门（gate）**：生物识别强认证控制「进入应用 UI」。密钥
 *   `alpha_biometric_gate` 绑定每次认证（auth-per-use），解锁时加密
 *   一段哨兵明文作为本次会话的门禁凭证——密钥只在生物识别通过的那
 *   一次操作里可用，伪造 UI 状态拿不到密钥操作；
 * - **静态加密（at-rest）**：`EncryptedKeyValueStore` 用独立的
 *   非认证绑定 keystore AES-GCM 密钥加密落盘值——门保护的是入口，
 *   加密保护的是数据本身（门被绕过/设备直接读盘的场景）。
 *   分开的理由：逐 tick 解密无法每次弹认证，auth-per-use 密钥
 *   不能用作批量数据密钥。
 *
 * 可测性切面：`BiometricCapabilities` / 状态机 / 加密存储全部接口化，
 * JVM 单测用注入的假能力与真实 AES 密钥跑；Keystore/Prompt 只在
 * 设备路径实例化（单测不触 Android 框架类）。
 */

// ---------- 能力探测（设备实现位 + 测试假件共用接口） ----------

/** 生物识别可用性 */
enum class GateAvailability {
    /** 可用且已录入（BIOMETRIC_WEAK 及以上） */
    Available,

    /** 无硬件或硬件不可用 */
    NoHardware,

    /** 有硬件但未录入凭据 */
    NoneEnrolled,
}

interface BiometricCapabilities {
    fun canAuthenticate(): GateAvailability
}

/** 设备实现：androidx.biometric（API 28+ 框架 BiometricPrompt，26/27 走兼容层） */
class AndroidBiometricCapabilities(context: Context) : BiometricCapabilities {
    private val manager = BiometricManager.from(context)

    override fun canAuthenticate(): GateAvailability =
        when (manager.canAuthenticate(BiometricManager.Authenticators.BIOMETRIC_WEAK)) {
            BiometricManager.BIOMETRIC_SUCCESS -> GateAvailability.Available
            BiometricManager.BIOMETRIC_ERROR_NONE_ENROLLED -> GateAvailability.NoneEnrolled
            else -> GateAvailability.NoHardware
        }
}

// ---------- 隐私设置（持久化 + 默认值） ----------

@Serializable
data class PrivacySettings(
    /** 生物识别门开关（默认关：opt-in，不抢首次启动体验） */
    val biometricEnabled: Boolean = false,
    /** 退后台即锁（默认开：金融应用的最小暴露面） */
    val lockOnBackground: Boolean = true,
    /** 防截屏/最近任务缩略图（FLAG_SECURE，默认开） */
    val screenshotShield: Boolean = true,
)

/** 设置存取：单键 JSON blob（kv 抽象复用 OfflineStore 的 KeyValueStore） */
class PrivacySettingsStore(private val kv: KeyValueStore) {
    fun load(): PrivacySettings {
        val raw = kv.get(SETTINGS_KEY) ?: return PrivacySettings()
        return try {
            Json.decodeFromString<PrivacySettings>(raw)
        } catch (e: Exception) {
            // 损坏即回默认（fail-safe 到最保守的锁屏开）
            PrivacySettings()
        }
    }

    fun save(settings: PrivacySettings) {
        kv.put(SETTINGS_KEY, Json.encodeToString(settings))
    }

    companion object {
        const val SETTINGS_KEY = "privacy_settings"
    }
}

// ---------- 门状态机（纯逻辑，JVM 可测） ----------

enum class LockState { Locked, Unlocked }

/**
 * 门状态机：迁移规则全部显式——
 * - 初始：biometricEnabled && lockOnBackground → Locked（会话冷启动即锁）；
 * - 退后台：lockOnBackground 时回 Locked 并清失败计数；
 * - 认证成功 → Unlocked；认证失败留在 Locked、失败计数累计；
 * - 设置更新：关掉 biometricEnabled 立即解锁（门的存在依赖开关）。
 */
class GateStateMachine(settings: PrivacySettings) {
    var settings: PrivacySettings = settings
        private set

    var state: LockState =
        if (settings.biometricEnabled && settings.lockOnBackground) LockState.Locked
        else LockState.Unlocked
        private set

    /** 连续认证失败次数（UI 提示面；不据此永久锁死——设备凭据自身有节流） */
    var failedAttempts: Int = 0
        private set

    fun onBackground() {
        if (settings.lockOnBackground) {
            state = LockState.Locked
            failedAttempts = 0
        }
    }

    fun onAuthSuccess() {
        state = LockState.Unlocked
        failedAttempts = 0
    }

    fun onAuthFailure() {
        failedAttempts += 1
    }

    fun updateSettings(new: PrivacySettings) {
        settings = new
        if (!new.biometricEnabled) {
            state = LockState.Unlocked
            failedAttempts = 0
        } else if (new.lockOnBackground && state == LockState.Unlocked) {
            // 开启「退后台即锁」后维持当前会话，下次退后台生效（不突袭锁定）
        }
    }
}

// ---------- 静态加密存储（AES-GCM，密钥注入；JVM 可测） ----------

/**
 * kv 包装层：put 时 AES-GCM 加密（每写一次新随机 12B IV），值 =
 * base64(IV ‖ ciphertext+tag)；get 解密，任何失败返回 null（防篡改
 * 读面）；delete/keys 透传。**只加密 value，key 明文**——键名用于
 * 检索，敏感语义由值承载（与 OfflineStore 的 quote:* 键约定一致）。
 */
class EncryptedKeyValueStore(
    private val inner: KeyValueStore,
    private val key: SecretKey,
) : KeyValueStore {
    override fun get(key: String): String? {
        val blob = inner.get(key) ?: return null
        return try {
            val raw = Base64.getDecoder().decode(blob)
            val iv = raw.copyOfRange(0, IV_LEN)
            val cipher = Cipher.getInstance(TRANSFORM)
            cipher.init(Cipher.DECRYPT_MODE, this.key, GCMParameterSpec(TAG_BITS, iv))
            String(cipher.doFinal(raw, IV_LEN, raw.size - IV_LEN), Charsets.UTF_8)
        } catch (e: Exception) {
            null
        }
    }

    override fun put(key: String, value: String) {
        val cipher = Cipher.getInstance(TRANSFORM)
        val iv = ByteArray(IV_LEN).also { java.security.SecureRandom().nextBytes(it) }
        cipher.init(Cipher.ENCRYPT_MODE, this.key, GCMParameterSpec(TAG_BITS, iv))
        val ct = cipher.doFinal(value.toByteArray(Charsets.UTF_8))
        inner.put(key, Base64.getEncoder().encodeToString(iv + ct))
    }

    override fun delete(key: String) {
        inner.delete(key)
    }

    override fun keys(): List<String> = inner.keys()

    companion object {
        const val TRANSFORM = "AES/GCM/NoPadding"
        const val IV_LEN = 12
        const val TAG_BITS = 128
    }
}

// ---------- 设备路径：keystore 双密钥 + BiometricPrompt（单测不触） ----------

/** keystore 密钥位：`gate` 为生物识别绑定（auth-per-use），`data` 为静态加密 */
object MobileKeys {
    private const val PROVIDER = "AndroidKeyStore"
    const val GATE_ALIAS = "alpha_biometric_gate"
    const val DATA_ALIAS = "alpha_offline_data"

    private fun keyStore(): KeyStore = KeyStore.getInstance(PROVIDER).apply { load(null) }

    /** 静态加密密钥：无认证绑定（存在即取，缺则生成 AES-256-GCM） */
    fun dataKey(): SecretKey {
        val ks = keyStore()
        (ks.getKey(DATA_ALIAS, null) as? SecretKey)?.let { return it }
        val gen = KeyGenerator.getInstance(
            KeyProperties.KEY_ALGORITHM_AES,
            PROVIDER,
        )
        gen.init(
            KeyGenParameterSpec.Builder(
                DATA_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return gen.generateKey()
    }

    /** 门密钥：生物识别强认证绑定（每次使用都需认证），新录入指纹即作废 */
    fun ensureGateKey() {
        val ks = keyStore()
        if (ks.containsAlias(GATE_ALIAS)) return
        val gen = KeyGenerator.getInstance(
            KeyProperties.KEY_ALGORITHM_AES,
            PROVIDER,
        )
        val spec = KeyGenParameterSpec.Builder(
            GATE_ALIAS,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .setUserAuthenticationRequired(true)
            .setInvalidatedByBiometricEnrollment(true)
        if (android.os.Build.VERSION.SDK_INT >= 30) {
            spec.setUserAuthenticationParameters(
                0,
                KeyProperties.AUTH_BIOMETRIC_STRONG,
            )
        } else {
            spec.setUserAuthenticationValidityDurationSeconds(-1)
        }
        gen.init(spec.build())
        gen.generateKey()
    }

    /** 门密钥对应的未初始化 Cipher（CryptoObject 携带，认证通过后框架内可用） */
    fun gateCipher(): Cipher {
        ensureGateKey()
        return Cipher.getInstance(EncryptedKeyValueStore.TRANSFORM)
    }

    /** 门密钥失效（新生物凭据录入/凭据清除）→ 删除重建，需重新 opt-in 门 */
    fun resetGateKeyIfInvalidated(e: Exception): Boolean {
        if (e is KeyPermanentlyInvalidatedException) {
            keyStore().deleteEntry(GATE_ALIAS)
            return true
        }
        return false
    }
}

/**
 * 生物识别认证流（设备路径）：CryptoObject 携带未初始化的门密钥
 * cipher——认证成功后回调里对哨兵明文做一次 doFinal，加密成功即证明
 * 「本次解锁事件确实发生了生物识别」（门禁凭证，可入会话审计）。
 */
fun promptBiometricGate(
    activity: FragmentActivity,
    title: String,
    subtitle: String,
    negativeText: String,
    onSuccess: (proof: String) -> Unit,
    onFailure: (message: String) -> Unit,
) {
    val cipher = try {
        MobileKeys.gateCipher()
    } catch (e: Exception) {
        onFailure("门密钥不可用: ${e.message}")
        return
    }
    val prompt = BiometricPrompt(
        activity,
        ContextCompat.getMainExecutor(activity),
        object : BiometricPrompt.AuthenticationCallback() {
            override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                try {
                    val working = result.cryptoObject?.cipher ?: cipher
                    val proof = Base64.getEncoder().encodeToString(
                        working.doFinal(GATE_PROOF.toByteArray(Charsets.UTF_8)),
                    )
                    onSuccess(proof)
                } catch (e: Exception) {
                    if (MobileKeys.resetGateKeyIfInvalidated(e)) {
                        onFailure("生物凭据已变更，请重新启用生物识别门")
                    } else {
                        onFailure("门密钥操作失败: ${e.message}")
                    }
                }
            }

            override fun onAuthenticationError(code: Int, errString: CharSequence) {
                onFailure(errString.toString())
            }

            override fun onAuthenticationFailed() {
                // 单次识别失败（未终止会话）：UI 层经状态机计数
                onFailure("recognize-failed")
            }
        },
    )
    val info = BiometricPrompt.PromptInfo.Builder()
        .setTitle(title)
        .setSubtitle(subtitle)
        .setNegativeButtonText(negativeText)
        .build()
    prompt.authenticate(info, BiometricPrompt.CryptoObject(cipher))
}

/** 门禁哨兵明文（加密成功 = 本次生物识别会话有效的可验证证据） */
const val GATE_PROOF = "alpha-gate-proof-v1"
