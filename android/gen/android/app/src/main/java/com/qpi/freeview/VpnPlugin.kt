package com.qpi.freeview

import android.app.Activity
import android.content.Intent
import android.net.VpnService
import androidx.activity.result.ActivityResult
import androidx.core.content.ContextCompat
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import app.tauri.plugin.Invoke

@InvokeArg
class ToggleVpnArgs {
    var mode: String = "TlsHeader5"
    var delayMs: Long = 2
}

@TauriPlugin
class VpnPlugin(private val activity: Activity) : Plugin(activity) {
    private var pendingMode: String = "TlsHeader5"
    private var pendingDelayMs: Long = 2

    @Command
    fun toggleVpn(invoke: Invoke) {
        val args = invoke.parseArgs(ToggleVpnArgs::class.java)
        if (FreeViewVpnService.isRunning) {
            val stopIntent = Intent(activity, FreeViewVpnService::class.java).apply {
                action = FreeViewVpnService.ACTION_STOP
            }
            activity.startService(stopIntent)

            val ret = JSObject()
            ret.put("isRunning", false)
            ret.put("port", 0)
            ret.put("error", null)
            invoke.resolve(ret)
            return
        }

        pendingMode = args.mode
        pendingDelayMs = args.delayMs

        val prepareIntent = VpnService.prepare(activity)
        if (prepareIntent != null) {
            startActivityForResult(invoke, prepareIntent, "onVpnPermissionResult")
        } else {
            startVpnService()
            val ret = JSObject()
            ret.put("isRunning", true)
            ret.put("port", 0)
            ret.put("error", null)
            invoke.resolve(ret)
        }
    }

    @ActivityCallback
    fun onVpnPermissionResult(invoke: Invoke, result: ActivityResult) {
        val ret = JSObject()
        if (result.resultCode == Activity.RESULT_OK) {
            startVpnService()
            ret.put("isRunning", true)
            ret.put("port", 0)
            ret.put("error", null)
        } else {
            ret.put("isRunning", false)
            ret.put("port", 0)
            ret.put("error", "VPN 권한이 거부되었습니다.")
        }
        invoke.resolve(ret)
    }

    private fun startVpnService() {
        val intent = Intent(activity, FreeViewVpnService::class.java).apply {
            action = FreeViewVpnService.ACTION_START
            putExtra(FreeViewVpnService.EXTRA_MODE, pendingMode)
            putExtra(FreeViewVpnService.EXTRA_DELAY_MS, pendingDelayMs)
        }
        ContextCompat.startForegroundService(activity, intent)
    }
}
