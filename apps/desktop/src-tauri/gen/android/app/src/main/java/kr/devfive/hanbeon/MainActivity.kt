package kr.devfive.hanbeon

import android.content.Intent
import android.os.Bundle
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge

/**
 * Tauri 웹뷰 Activity.
 *
 * 설정 칸(오버레이)을 누르면 OverlayService가 extra `screen=settings`를 담아
 * 이 액티비티를 다시 띄운다. 그때 웹뷰의 URL을 settings/로 바꿔 설정 화면을
 * 보여주고, extra 없이 시작되면 floating 컨트롤러(index.html)로 돌아간다.
 *
 * wry가 만든 RustWebView는 onWebViewCreate()로 전달되므로 여기서 참조를 저장해
 * 나중에 loadUrlMainThread()로 이동할 수 있게 한다.
 */
class MainActivity : TauriActivity() {
  private var webView: WebView? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    navigateByIntent(intent)
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    setIntent(intent)
    navigateByIntent(intent)
  }

  override fun onWebViewCreate(rustWebView: WebView) {
    super.onWebViewCreate(rustWebView)
    this.webView = rustWebView
    navigateByIntent(intent)
  }

  private fun navigateByIntent(intent: Intent?) {
    val screen = intent?.getStringExtra(EXTRA_SCREEN) ?: return
    if (screen != "settings") return
    intent.removeExtra(EXTRA_SCREEN)
    // URL을 바꾸지 않고 프론트에 알린다. floating 컨트롤러가 설정 오버레이를
    // 겹쳐 렌더하고, 닫으면 스캔 상태가 그대로 유지된다.
    (webView as? RustWebView)?.let { wv ->
      wv.post {
        wv.evaluateJavascript(
          "window.dispatchEvent(new CustomEvent('hanbeon://open-settings'))",
          null,
        )
      }
    }
  }

  companion object {
    const val EXTRA_SCREEN = "screen"
  }
}
