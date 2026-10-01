package com.alpha.finance.mobile

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable

/**
 * 主题适配（L511 全平台深色模式与系统主题：Android 落地面）。
 *
 * 三态偏好语义对齐 web 端 ThemeToggle（L432 `web/app/src/lib/theme.ts`：
 * system/light/dark → 生效主题，system 跟随系统深色检测）；桌面端复用
 * 同一 web 主题系统（tauri 窗口 chrome 走 OS 默认）。偏好持久化单键
 * 值（枚举名），损坏/缺省回 SYSTEM（跟随系统是三端一致的缺省口径）。
 *
 * 解析/映射全部纯函数（JVM 单测锁定）；Composable 只做 Material3
 * light/dark 配色切换（骨架期用默认配色板，与既有 lightColorScheme()
 * 一致，品牌色板归后续设计项）。偏好变更重启生效（读点在 onCreate）。
 */

/** 主题偏好三态 */
enum class ThemePreference { SYSTEM, LIGHT, DARK }

/** 偏好解析收口：任意字符串 → ThemePreference（缺省/未知 → SYSTEM） */
fun parseThemePreference(raw: String?): ThemePreference = when (raw) {
    "LIGHT" -> ThemePreference.LIGHT
    "DARK" -> ThemePreference.DARK
    "SYSTEM" -> ThemePreference.SYSTEM
    else -> ThemePreference.SYSTEM
}

/** 偏好 → 是否深色（语义对齐 web resolveTheme：system 检测系统，其余直取） */
fun prefersDark(pref: ThemePreference, systemDark: Boolean): Boolean = when (pref) {
    ThemePreference.SYSTEM -> systemDark
    ThemePreference.LIGHT -> false
    ThemePreference.DARK -> true
}

/** 偏好持久化（kv 抽象复用 OfflineStore 的 KeyValueStore） */
class ThemeSettingsStore(private val kv: KeyValueStore) {
    fun load(): ThemePreference = parseThemePreference(kv.get(KEY))

    fun save(pref: ThemePreference) {
        kv.put(KEY, pref.name)
    }

    companion object {
        const val KEY = "theme_preference"
    }
}

/** 应用主题：按偏好切 Material3 配色（系统档跟随 isSystemInDarkTheme） */
@Composable
fun AlphaTheme(
    pref: ThemePreference,
    content: @Composable () -> Unit,
) {
    val dark = prefersDark(pref, isSystemInDarkTheme())
    MaterialTheme(
        colorScheme = if (dark) darkColorScheme() else lightColorScheme(),
        content = content,
    )
}
