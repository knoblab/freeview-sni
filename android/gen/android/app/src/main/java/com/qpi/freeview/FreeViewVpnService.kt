package com.qpi.freeview

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.VpnService
import android.os.Build
import android.os.ParcelFileDescriptor

class FreeViewVpnService : VpnService() {
    companion object {
        const val ACTION_START = "com.qpi.freeview.START_VPN"
        const val ACTION_STOP = "com.qpi.freeview.STOP_VPN"
        const val EXTRA_MODE = "mode"
        const val EXTRA_DELAY_MS = "delay_ms"
        private const val CHANNEL_ID = "freeview_vpn_channel"
        private const val NOTIFICATION_ID = 1001

        @Volatile
        var isRunning: Boolean = false
            private set

        init {
            System.loadLibrary("freeview_lib")
        }
    }

    private var vpnInterface: ParcelFileDescriptor? = null

    private external fun nativeStartTun(tunFd: Int, mode: String, delayMs: Long): Boolean
    private external fun nativeStopTun()

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                stopVpn()
                return START_NOT_STICKY
            }
            ACTION_START, null -> {
                val mode = intent?.getStringExtra(EXTRA_MODE) ?: "TlsHeader5"
                val delayMs = intent?.getLongExtra(EXTRA_DELAY_MS, 2L) ?: 2L
                startVpn(mode, delayMs)
                return START_STICKY
            }
        }
        return START_NOT_STICKY
    }

    private fun startVpn(mode: String, delayMs: Long) {
        if (isRunning) return

        createNotificationChannel()
        val notification = buildNotification()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(
                NOTIFICATION_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE
            )
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }

        val builder = Builder()
            .setSession("FreeView")
            .setMtu(1500)
            .addAddress("10.0.0.2", 32)
            .addRoute("0.0.0.0", 0)
            .addDnsServer("1.1.1.1")
            .addDnsServer("8.8.8.8")

        try {
            builder.addDisallowedApplication(packageName)
        } catch (_: Exception) {
        }

        val pfd = builder.establish()
        if (pfd == null) {
            stopSelf()
            return
        }

        val detachedFd = pfd.detachFd()
        val ok = nativeStartTun(detachedFd, mode, delayMs)
        if (ok) {
            isRunning = true
        } else {
            stopVpn()
        }
    }

    private fun stopVpn() {
        nativeStopTun()
        try {
            vpnInterface?.close()
        } catch (_: Exception) {
        }
        vpnInterface = null
        isRunning = false
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onRevoke() {
        stopVpn()
        super.onRevoke()
    }

    override fun onDestroy() {
        stopVpn()
        super.onDestroy()
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                "FreeView VPN 보호",
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = "SNI 검열 우회 VPN 서비스 상태"
            }
            val manager = getSystemService(NotificationManager::class.java)
            manager?.createNotificationChannel(channel)
        }
    }

    private fun buildNotification(): Notification {
        val openIntent = Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP
        }
        val pendingOpen = PendingIntent.getActivity(
            this,
            0,
            openIntent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )

        val stopIntent = Intent(this, FreeViewVpnService::class.java).apply {
            action = ACTION_STOP
        }
        val pendingStop = PendingIntent.getService(
            this,
            1,
            stopIntent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )

        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(this, CHANNEL_ID)
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(this)
        }

        return builder
            .setContentTitle("FreeView 보호 활성화")
            .setContentText("SNI 우회 VPN이 동작 중입니다.")
            .setSmallIcon(android.R.drawable.ic_lock_lock)
            .setContentIntent(pendingOpen)
            .setOngoing(true)
            .addAction(
                Notification.Action.Builder(
                    null,
                    "중지",
                    pendingStop
                ).build()
            )
            .build()
    }
}
