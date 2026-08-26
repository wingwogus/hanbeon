package kr.devfive.hanbeon

import android.content.Intent
import android.os.Bundle
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge

/**
 * Tauri 웹뷰 Activity. 앱의 기본 화면은 설정이다.
 *
 * 오버레이 서비스가 extra `screen`으로 어느 화면을 보여줄지 정한다:
 * - "floating" → floating 컨트롤러(index.html)
 * - 그 외/없음  → 설정 화면(settings/index.html)
 *
 * wry가 만든 RustWebView는 onWebViewCreate()로 전달되므로 여기서 참조를 저장해
 * 나중에 loadUrlMainThread()로 화면을 전환할 수 있게 한다.
 */
class MainActivity : TauriActivity() {
  private var webView: WebView? = null
  private var currentScreen: String? = null

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
    val screen = intent?.getStringExtra(EXTRA_SCREEN) ?: "settings"
    intent?.removeExtra(EXTRA_SCREEN)
    if (screen == currentScreen) return

    // wry는 상대경로를 그대로 loadUrl하면 DNS 조회로 실패한다. 반드시
    // tauri asset origin 절대 URL을 써야 한다.
    val target =
      when (screen) {
        "floating" -> "http://tauri.localhost/index.html"
        else -> "http://tauri.localhost/settings/"
      }
    (webView as? RustWebView)?.loadUrlMainThread(target) ?: return
    currentScreen = screen
  }

  companion object {
    const val EXTRA_SCREEN = "screen"
  }
}
