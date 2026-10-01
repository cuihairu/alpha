package com.alpha.finance.mobile

import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Test

/**
 * 主题适配单测（JVM 级，L511 docs/theme-adaptation.md §4）：三态解析
 * 收口、偏好→深色映射（语义对齐 web resolveTheme）、持久化往返。
 * Composable 配色切换不在此覆盖（无 Compose 测试面）。
 */
class ThemeTest {

    @Test
    fun `解析收口：合法三态直取，未知或缺省回跟随系统`() {
        assertSame(ThemePreference.LIGHT, parseThemePreference("LIGHT"))
        assertSame(ThemePreference.DARK, parseThemePreference("DARK"))
        assertSame(ThemePreference.SYSTEM, parseThemePreference("SYSTEM"))
        assertSame(ThemePreference.SYSTEM, parseThemePreference("dark"))
        assertSame(ThemePreference.SYSTEM, parseThemePreference("mystery"))
        assertSame(ThemePreference.SYSTEM, parseThemePreference(null))
    }

    @Test
    fun `偏好到深色映射：system 检测系统，其余直取（对齐 web resolveTheme）`() {
        assertSame(ThemePreference.SYSTEM, parseThemePreference("SYSTEM"))
        // system 跟随系统深色检测
        assertEquals(true, prefersDark(ThemePreference.SYSTEM, systemDark = true))
        assertEquals(false, prefersDark(ThemePreference.SYSTEM, systemDark = false))
        // 显式档不受系统影响
        assertEquals(false, prefersDark(ThemePreference.LIGHT, systemDark = true))
        assertEquals(true, prefersDark(ThemePreference.DARK, systemDark = false))
    }

    @Test
    fun `持久化往返：枚举名单键存取，损坏回跟随系统`() {
        val kv = InMemoryKeyValueStore()
        val store = ThemeSettingsStore(kv)
        assertSame(ThemePreference.SYSTEM, store.load())
        store.save(ThemePreference.DARK)
        assertSame(ThemePreference.DARK, store.load())
        store.save(ThemePreference.LIGHT)
        assertSame(ThemePreference.LIGHT, store.load())
        // 损坏值 → SYSTEM（跟随系统缺省口径）
        kv.put(ThemeSettingsStore.KEY, "{broken")
        assertSame(ThemePreference.SYSTEM, store.load())
    }
}
