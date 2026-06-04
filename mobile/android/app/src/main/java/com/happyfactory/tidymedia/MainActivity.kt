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
    val src = defaultDcim()
    val out = defaultOutput()

    Column(
        modifier = modifier.fillMaxSize().padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text("tidymedia · Android (P0+P1)", style = MaterialTheme.typography.titleLarge)
        Text("Source: $src")
        Text("Output: $out")
        Button(onClick = {
            scope.launch {
                status = "Running dry-run…"
                status = runDryRun(src, out)
            }
        }) { Text("Run dry-run") }
        Text(status, style = MaterialTheme.typography.bodyMedium)
    }
}

private fun defaultDcim(): String =
    Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DCIM).absolutePath

// tidymedia dry-run 不写文件，正式 run 落在 Documents/tidymedia-out 避免污染相册。
// Real SAF 接入后这里换成用户选定的 DocumentFile path。
private fun defaultOutput(): String =
    Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOCUMENTS).absolutePath + "/tidymedia-out"

private suspend fun runDryRun(src: String, out: String): String = withContext(Dispatchers.IO) {
    try {
        val stats: TidyStats = tidyDryRun(src = src, output = out)
        "${stats.status} (scanned=${stats.totalScanned}, copied=${stats.copied})"
    } catch (e: TidyException) {
        "Error: ${e.message ?: "<no message>"}"
    } catch (e: Throwable) {
        "Crash: ${e::class.simpleName}: ${e.message ?: ""}"
    }
}
