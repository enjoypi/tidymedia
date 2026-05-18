package com.happyfactory.tidymedia

import android.os.Bundle
import android.os.Environment
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.tidymedia.TidyException
import uniffi.tidymedia.TidyStats
import uniffi.tidymedia.tidyDryRun
import uniffi.tidymedia.tidymediaVersion

// 最小 Compose 壳：单页面，调 Rust core 跑 dry-run 并显示结果。
// SAF 实际接入留到 P1.5 —— 现在固定扫 DCIM，验证 JNI 链路即可。
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                Scaffold { padding ->
                    TidyScreen(modifier = Modifier.padding(padding))
                }
            }
        }
    }
}

@Composable
fun TidyScreen(modifier: Modifier = Modifier) {
    var status by remember { mutableStateOf<String>("Rust core v${tidymediaVersion()}") }
    val scope = rememberCoroutineScope()

    Column(
        modifier = modifier.fillMaxSize().padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text("tidymedia · Android (P0+P1)", style = MaterialTheme.typography.titleLarge)
        Text("Source: ${defaultDcim()}")
        Text("Output: ${defaultOutput()}")
        Button(onClick = {
            scope.launch {
                status = "Running dry-run…"
                status = runDryRun()
            }
        }) { Text("Run dry-run") }
        Text(status, style = MaterialTheme.typography.bodyMedium)
    }
}

private fun defaultDcim(): String =
    Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DCIM).absolutePath

private fun defaultOutput(): String {
    // tidymedia dry-run 不写文件，正式 run 落在 Documents/tidymedia-out 避免污染相册。
    // Real SAF 接入后这里换成用户选定的 DocumentFile path。
    return Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOCUMENTS).absolutePath + "/tidymedia-out"
}

private suspend fun runDryRun(): String = withContext(Dispatchers.IO) {
    try {
        val src = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DCIM).absolutePath
        val out = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOCUMENTS).absolutePath + "/tidymedia-out"
        val stats: TidyStats = tidyDryRun(src = src, output = out)
        "${stats.status} (scanned=${stats.totalScanned}, copied=${stats.copied})"
    } catch (e: TidyException) {
        "Error: ${e.message ?: "<no message>"}"
    } catch (e: Throwable) {
        "Crash: ${e::class.simpleName}: ${e.message ?: ""}"
    }
}
