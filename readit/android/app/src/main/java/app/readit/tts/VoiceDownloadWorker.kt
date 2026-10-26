package app.readit.tts

import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context
import android.content.pm.ServiceInfo
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.work.CoroutineWorker
import androidx.work.Data
import androidx.work.ExistingWorkPolicy
import androidx.work.ForegroundInfo
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import app.readit.R
import app.readit.ReaditApp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** Downloads a voice pack in the background with a progress notification; survives leaving the app. */
class VoiceDownloadWorker(ctx: Context, params: WorkerParameters) : CoroutineWorker(ctx, params) {

    override suspend fun doWork(): Result {
        val packId = inputData.getString(KEY_PACK) ?: return Result.failure()
        val vm = ReaditApp.instance.voices
        val pack = vm.catalog.pack(packId) ?: return Result.failure()
        setForeground(foregroundInfo(pack.title))
        return withContext(Dispatchers.IO) {
            try {
                vm.install(packId)
                Result.success()
            } catch (e: Exception) {
                Result.failure(Data.Builder().putString("error", e.message).build())
            }
        }
    }

    override suspend fun getForegroundInfo(): ForegroundInfo = foregroundInfo("voice")

    private fun foregroundInfo(title: String): ForegroundInfo {
        val nm = applicationContext.getSystemService(NotificationManager::class.java)
        if (Build.VERSION.SDK_INT >= 26) {
            nm.createNotificationChannel(NotificationChannel(CHANNEL, "Voice downloads", NotificationManager.IMPORTANCE_LOW))
        }
        val n = NotificationCompat.Builder(applicationContext, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle("Downloading $title")
            .setContentText("READIT is saving this voice for offline use.")
            .setOngoing(true)
            .setProgress(0, 0, true)
            .build()
        return if (Build.VERSION.SDK_INT >= 29) {
            ForegroundInfo(NOTIFICATION_ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } else {
            ForegroundInfo(NOTIFICATION_ID, n)
        }
    }

    companion object {
        private const val KEY_PACK = "pack"
        private const val CHANNEL = "voice-downloads"
        private const val NOTIFICATION_ID = 4101

        fun start(context: Context, packId: String) {
            val req = OneTimeWorkRequestBuilder<VoiceDownloadWorker>()
                .setInputData(Data.Builder().putString(KEY_PACK, packId).build())
                .addTag("voice:$packId")
                .build()
            WorkManager.getInstance(context).enqueueUniqueWork("voice:$packId", ExistingWorkPolicy.KEEP, req)
        }

        /** Pausing keeps the partial file so the next start resumes from the same byte. */
        fun pause(context: Context, packId: String) {
            ReaditApp.instance.voices.requestPause(packId)
            WorkManager.getInstance(context).cancelUniqueWork("voice:$packId")
        }
    }
}
