package com.alpha.finance.mobile

import android.content.Context
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import java.util.concurrent.TimeUnit

/**
 * 推送/同步的壳层接缝（L337，docs/mobile-push-sync.md §3/§7）：Rust 侧只做
 * 决策（推什么/何时同步），本文件把决策落到 Android 平台能力上。骨架期给出
 * 接口面 + WorkManager 装配口径 + Worker 决策回路；通知实际弹出
 * （NotificationChannel 注册、POST_NOTIFICATIONS 授权）与 Worker 内真实取数
 * 归 L339 / 真机 TODO（文档 §10）。
 */

/**
 * 通知送达口：`AlphaBridge.takePending()` 取回的 [NotificationSpecPayload]
 * 逐条交实现方弹平台通知。骨架不提供实现（无真机验收面）；真机实现要点：
 * - Android O+ 先 `NotificationManager.createNotificationChannel`（渠道 id 建议
 *   `price_alerts`，IMPORTANCE_DEFAULT）；
 * - Android 13+ 动态申请 POST_NOTIFICATIONS，被拒时降级为应用内提示条；
 * - 以 `spec.id` 作通知 id 去重（Rust 侧已按规则穿越去重，此处只防重绘）；
 * - App 前台时改发应用内提示，不弹系统通知。
 */
interface NotificationDispatcher {
    /** 送一条通知；返回是否被平台接受（失败由调用方决定降级，骨架不重试） */
    fun dispatch(spec: NotificationSpecPayload): Boolean
}

/**
 * 周期后台同步 Worker：把 Rust 侧间隔闸门映射进 WorkManager。回调序列——
 * `syncPlan("periodic")` → due 则平台取数（L339 前空转）→ `markSynced()`。
 * 持桥装配（App 级 AlphaBridge + WorkerFactory 注入）随 L339 离线存储一起做：
 * MainActivity 的桥随 Activity 生命周期 close（L301 口径），后台任务需要
 * App 级桥，骨架期不发明第二套生命周期。
 */
class PeriodicSyncWorker(
    context: Context,
    params: WorkerParameters,
) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        // 桥注入前的骨架期：不触 FFI，直接成功（闸门与真实取数归 L339 装配）。
        // 装配后的完整回路（Rust 侧决策，壳层只执行）：
        //   val plan = bridge.syncPlan(AlphaBridge.TRIGGER_PERIODIC)
        //   if (plan.due) { /* L339 平台取数 */ bridge.markSynced() }
        return Result.success()
    }
}

/**
 * WorkManager 装配口径（App 启动时调用一次；KEEP 策略避免重复排队）。
 *
 * **钳制口径（文档假设③）**：WorkManager 周期任务系统下限 15min，Rust 默认
 * `interval_secs=300` 只作核心库自身闸门——壳层用 [MIN_PERIODIC_MINUTES] 装配，
 * 实际周期 ≥ 核心间隔即兼容（闸门放行时才真正出 due 计划）。
 */
object PushSyncSeam {
    /** WorkManager 周期任务下限（系统钳制）；核心库 300s 闸门由 FFI 侧把守 */
    const val MIN_PERIODIC_MINUTES: Long = 15L

    /** 真机验证归文档 §10②（Doze/App Standby 下的实际触发频率） */
    fun schedulePeriodicSync(context: Context) {
        val request = PeriodicWorkRequestBuilder<PeriodicSyncWorker>(
            MIN_PERIODIC_MINUTES, TimeUnit.MINUTES
        ).setConstraints(
            Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()
        ).build()
        WorkManager.getInstance(context).enqueueUniquePeriodicWork(
            "alpha-periodic-sync",
            ExistingPeriodicWorkPolicy.KEEP,
            request,
        )
    }
}
