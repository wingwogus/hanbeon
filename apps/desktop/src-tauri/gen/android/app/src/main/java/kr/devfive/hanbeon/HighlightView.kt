package kr.devfive.hanbeon

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Rect
import android.util.TypedValue
import android.view.View

/**
 * 지금 고르고 있는 요소에 테두리를 그린다.
 *
 * 안드로이드는 접근성 포커스를 눈에 보이게 그려 주지 않는다. `ACTION_ACCESSIBILITY_FOCUS`가
 * 옮기는 것은 스크린리더용 보이지 않는 커서라, 브라우저의 포커스 링도 생기지 않는다.
 * 그래서 우리가 직접 그린다.
 *
 * 없으면 제품이 성립하지 않는다. 사용자는 무엇이 선택돼 있는지 봐야 누를지 말지를
 * 정한다(PRD 원칙 2·6, F7).
 */
class HighlightView(context: Context) : View(context) {
    private var target: Rect? = null

    private val border =
        Paint(Paint.ANTI_ALIAS_FLAG).apply {
            style = Paint.Style.STROKE
            // 색만으로 알리지 않는다(원칙 6). 두께로도 확실히 구분되게 굵게 그린다.
            strokeWidth = dp(4f)
            color = Color.parseColor("#0E7A63")
        }

    /** 바깥 흰 선. 어두운 화면에서도 초록 테두리가 묻히지 않게 한다. */
    private val halo =
        Paint(Paint.ANTI_ALIAS_FLAG).apply {
            style = Paint.Style.STROKE
            strokeWidth = dp(7f)
            color = Color.parseColor("#B3FFFFFF")
        }

    fun show(bounds: Rect?) {
        target = bounds
        postInvalidate()
    }

    override fun onDraw(canvas: Canvas) {
        val rect = target ?: return
        val radius = dp(6f)

        // `getBoundsInScreen()`은 화면 절대 좌표(상태바 포함)로 온다. 그런데 이 뷰의
        // 원점은 창에 걸린 인셋만큼 밀려 있을 수 있어서, 그대로 그리면 그 높이만큼
        // 어긋난다. 실제로 상태바 높이만큼 아래에 그려졌다.
        //
        // 인셋이 무엇 때문인지 짐작하지 않고 뷰에게 자기 위치를 물어 보정한다.
        val origin = IntArray(2)
        getLocationOnScreen(origin)
        canvas.save()
        canvas.translate(-origin[0].toFloat(), -origin[1].toFloat())
        canvas.drawRoundRect(
            rect.left.toFloat(),
            rect.top.toFloat(),
            rect.right.toFloat(),
            rect.bottom.toFloat(),
            radius,
            radius,
            halo,
        )
        canvas.drawRoundRect(
            rect.left.toFloat(),
            rect.top.toFloat(),
            rect.right.toFloat(),
            rect.bottom.toFloat(),
            radius,
            radius,
            border,
        )
        canvas.restore()
    }

    private fun dp(value: Float): Float =
        TypedValue.applyDimension(
            TypedValue.COMPLEX_UNIT_DIP,
            value,
            resources.displayMetrics,
        )
}
