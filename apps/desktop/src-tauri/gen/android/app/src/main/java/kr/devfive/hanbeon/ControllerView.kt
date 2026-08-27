package kr.devfive.hanbeon

import android.content.Context
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.style.RelativeSizeSpan
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.TextView
import org.json.JSONArray
import org.json.JSONObject

/**
 * 4칸 컨트롤러 — 데스크톱과 같은 자리, 압축된 높이(PRD F1).
 *
 * 자리는 원형 그대로다. 왼쪽에 이동 둘이 세로로, 오른쪽에 선택이 그 높이만큼,
 * 아래에 앱별 칸과 설정. 폰 화면은 세로 공간이 비싸므로 칸 높이를 크게 줄였다.
 * 접근성 서비스는 컨트롤러에 가려 두드릴 수 없는 요소를 스캔 대상에서 빼 버리므로,
 * 가림을 줄이는 것이 곧 닿을 수 있는 화면을 늘리는 것이다.
 *
 *     [ > ] [ ↵ ]
 *     [ < ]      ← 메인 블록, 항상 네 자리
 *     [앱별 칸 …] ← 프리셋이 붙을 때만
 *     [   ⚙   ]
 *     [상태 줄]   ← 알릴 것이 없어도 비우지 않는다(PRD F5)
 *
 * 선택과 설정은 `↵`·`⚙` 기호로 그린다. 무엇을 그릴지는 코어가 보낸 상태가 정하고,
 * 여기서 커서를 옮기거나 순서를 만들지 않는다.
 */
class ControllerView(context: Context) : LinearLayout(context) {
    private val moves = LinearLayout(context)
    private val extras = LinearLayout(context)
    private val status = TextView(context)
    private val enter: TextView

    /** 칸 이름(코어의 label) → 그 칸을 그리는 뷰. 커서 위치를 표시할 때 쓴다. */
    private val cells = mutableMapOf<String, TextView>()

    /** 지금 정지 중인가. 조정 이유 표시가 끝난 뒤 무엇으로 돌아갈지 정한다. */
    private var paused = false

    init {
        orientation = VERTICAL
        setPadding(pad(4), pad(4), pad(4), pad(4))
        setPaused(false)

        moves.orientation = VERTICAL
        moves.layoutParams = LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)
        moves.addView(cell(">"))
        moves.addView(cell("<", topGapDp = 4))

        enter = cell("Enter", textSizeSp = ENTER_SP)
        enter.layoutParams =
            LayoutParams(0, pad(MAIN_HEIGHT_DP), 1f).also {
                it.marginStart = pad(4)
            }

        val top =
            LinearLayout(context).apply {
                orientation = HORIZONTAL
                layoutParams =
                    LayoutParams(
                        ViewGroup.LayoutParams.MATCH_PARENT,
                        ViewGroup.LayoutParams.WRAP_CONTENT,
                    )
            }
        top.addView(moves)
        top.addView(enter)
        addView(top)

        // 앱별 칸 행. 없을 때는 아무 자리도 차지하지 않는다.
        extras.orientation = VERTICAL
        extras.layoutParams =
            LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
        addView(extras)

        val settings = cell("설정", heightDp = CELL_HEIGHT_DP, topGapDp = 4)
        addView(settings)

        // 알릴 것이 없어도 비우지 않는다. 나타났다 사라지는 줄은 그때마다 칸을
        // 밀어 올려 사용자가 커서 위치를 다시 찾게 만든다(PRD F5).
        status.gravity = Gravity.CENTER
        status.setTextColor(Color.parseColor("#5A6468"))
        status.setTextSize(TypedValue.COMPLEX_UNIT_SP, 13f)
        status.setPadding(0, pad(4), 0, 0)
        status.includeFontPadding = false
        status.text = "준비 중"
        addView(status)
    }

    /** 코어가 보낸 상태를 그린다. */
    fun render(json: String) {
        val state = runCatching { JSONObject(json) }.getOrNull() ?: return
        val list = state.optJSONArray("cells") ?: return
        val cursor = state.optInt("cursor", -1)

        // 정지 중에는 어떤 칸도 커서를 갖지 않는다. 멈춘 채로 한 칸이 강조되어
        // 있으면 스캔 중과 구분되지 않아, 사용자는 기다려야 할지 눌러야 할지
        // 알 수 없다. 데스크톱도 같은 규칙이다.
        val paused = state.optString("mode") == "paused"
        if (paused != this.paused) {
            this.paused = paused
            setPaused(paused)
        }

        rebuildExtras(list)

        for (i in 0 until list.length()) {
            val cell = list.optJSONObject(i) ?: continue
            val label = cell.optString("label")
            val view = cells[label] ?: continue
            val here = !paused && i == cursor
            paint(view, here)
            // 기호만 보인다가, 커서가 온 칸에만 이름을 붙인다. 모든 칸에 설명을
            // 달면 글자가 늘어 정작 커서 위치가 묻힌다(PRD F7).
            view.text =
                if (here) labeled(GLYPHS[label] ?: label, cell.optString("name"))
                else GLYPHS[label] ?: label
        }

        if (paused) {
            // 멈춰 있다는 사실이 조정 이유나 현재 속도보다 앞선다.
            status.text = "일시정지 — 길게 눌러 다시 시작"
            status.setTextColor(WARNING)
            return
        }

        val interval = state.optLong("intervalMs", 0)
        if (interval > 0 && status.tag == null) {
            status.text = "%.1f초마다".format(interval / 1000.0)
            status.setTextColor(Color.parseColor("#5A6468"))
        }
    }

    /**
     * 정지를 색 말고도 알린다(원칙 6).
     *
     * 창 테두리를 굵은 경고색으로 바꾼다. 색만으로 알리면 저시력 사용자와
     * 색각 이상 사용자에게 정지와 스캔이 같은 화면이 된다.
     */
    private fun setPaused(paused: Boolean) {
        background =
            GradientDrawable().apply {
                cornerRadius = pad(16).toFloat()
                setColor(Color.parseColor("#F5F7F7"))
                if (paused) {
                    setStroke(pad(5), WARNING)
                } else {
                    setStroke(pad(1), Color.parseColor("#CFD8D8"))
                }
            }
    }

    /** 간격이 바뀐 이유를 6초간 띄운다. 그 뒤에는 현재 속도로 돌아간다(PRD F5). */
    fun notice(json: String) {
        val reason = runCatching { JSONObject(json).optString("reason") }.getOrNull() ?: return
        if (reason.isEmpty()) return
        status.text = reason
        status.setTextColor(Color.parseColor("#0E7A63"))
        status.tag = true
        postDelayed({
            status.tag = null
            // 이 사이에 정지로 내려갔으면 정지 안내를 지우지 않는다. 멈춘 것을
            // 모르게 만드는 쪽이 조정 이유를 놓치는 것보다 나쁜다.
            if (paused) {
                status.text = "일시정지 — 길게 눌러 다시 시작"
                status.setTextColor(WARNING)
            } else {
                status.setTextColor(Color.parseColor("#5A6468"))
            }
        }, NOTICE_MS)
    }

    /** 앱별 칸이 사라졌을 때 자리를 비운다. */
    fun fitCells(count: Int) {
        if (count == 0) {
            extras.removeAllViews()
            cells.keys.retainAll(setOf(">", "<", "Enter", "설정"))
        }
    }

    private fun rebuildExtras(list: JSONArray) {
        val wanted = mutableListOf<Pair<String, String>>()
        for (i in 0 until list.length()) {
            val cell = list.optJSONObject(i) ?: continue
            if (cell.optString("kind") == "extra") {
                wanted.add(cell.optString("label") to cell.optString("name"))
            }
        }
        if (wanted.size == extras.childCount) return

        extras.removeAllViews()
        wanted.forEach { (label, _) ->
            // 앱별 칸은 기본 칸과 다르게 그린다. 지금 앱에서만 쓸 수 있고 앱이
            // 바뀌면 사라지는 칸이라, 같아 보이면 사용자가 자리를 외운다(PRD F11).
            extras.addView(cell(label, heightDp = EXTRA_HEIGHT_DP, textSizeSp = 15f, topGapDp = 4, accent = true))
        }
    }

    private fun paint(
        view: TextView,
        here: Boolean,
    ) {
        val accent = view.getTag(ACCENT_KEY) == true
        val fill = if (here) "#0E7A63" else if (accent) "#EFF6F4" else "#FFFFFF"
        val line = if (here) "#0E7A63" else if (accent) "#9CC7BC" else "#D8E0E0"
        view.setTextColor(if (here) Color.WHITE else Color.parseColor("#1B2124"))
        view.background =
            GradientDrawable().apply {
                cornerRadius = pad(if (accent) 14 else 10).toFloat()
                setColor(Color.parseColor(fill))
                // 색만으로 구분하지 않는다. 커서가 있으면 테두리도 굵어진다(원칙 6).
                setStroke(pad(if (here) 5 else 2), Color.parseColor(line))
            }
    }

    /**
     * 칸 하나. 폭은 부모가 채우고, 높이는 못 박는다.
     *
     * 높이를 고정하는 이유는 커서가 온 칸만 두 줄(`↵`\n`선택`)이 되는데, 높이가
     * 내용을 따라가면 아래 칸이 밀려 움직임 자체가 노이즈가 되기 때문이다.
     */
    private fun cell(
        label: String,
        heightDp: Int = CELL_HEIGHT_DP,
        textSizeSp: Float = SYMBOL_SP,
        topGapDp: Int = 0,
        accent: Boolean = false,
    ): TextView {
        val view =
            TextView(context).apply {
                text = GLYPHS[label] ?: label
                gravity = Gravity.CENTER
                setTextSize(TypedValue.COMPLEX_UNIT_SP, textSizeSp)
                // 줄 위아래의 빈 여백을 없애야 좁은 칸에 두 줄이 온전히 든다.
                includeFontPadding = false
                setTag(ACCENT_KEY, accent)
                layoutParams =
                    LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, pad(heightDp)).also {
                        it.topMargin = pad(topGapDp)
                    }
            }
        paint(view, false)
        cells[label] = view
        return view
    }

    /**
     * 커서가 온 칸의 두 줄 텍스트. 기호는 그대로, 이름은 절반 크기로.
     *
     * 이름까지 기호 크기면 "한 번 더 누르면 되돌아옴" 같은 이름이 좁은 칸에서
     * 네 줄로 부서진다. 크기를 나눠야 한 줄 정보가 두 줄로 유지된다.
     */
    private fun labeled(
        glyph: String,
        name: String,
    ): CharSequence =
        SpannableStringBuilder(glyph).apply {
            if (name.isEmpty()) return@apply
            append("\n")
            append(name)
            setSpan(
                RelativeSizeSpan(NAME_SHRINK),
                glyph.length + 1,
                length,
                Spanned.SPAN_EXCLUSIVE_EXCLUSIVE,
            )
        }

    private fun pad(dp: Int): Int =
        TypedValue.applyDimension(
            TypedValue.COMPLEX_UNIT_DIP,
            dp.toFloat(),
            resources.displayMetrics,
        ).toInt()

    private companion object {
        /** 코어의 label → 화면에 그릴 기호. 없는 라벨은 label을 그대로 쓴다. */
        val GLYPHS = mapOf("Enter" to "↵", "설정" to "⚙")

        /** 이동 칸 하나의 높이. 두 줄(기호+이름)이 들어갈 자리를 항상 예약한다. */
        const val CELL_HEIGHT_DP = 48

        /** 선택 칸 = 이동 칸 둘 + 사이 간격. 원형과 같은 규칙이다. */
        const val MAIN_HEIGHT_DP = CELL_HEIGHT_DP * 2 + 4

        /** 앱별 칸 행의 높이. */
        const val EXTRA_HEIGHT_DP = 34

        /** 기호 크기. ⚙는 획이 얇아 작으면 소멸한다. */
        const val SYMBOL_SP = 26f

        /** 선택 칸의 기호 크기. 가장 자주 누르는 칸이므로 멀리서도 읽히게 크게 둔다. */
        const val ENTER_SP = 38f

        /** 커서 칸에서 이름이 기호에 비해 줄어드는 배수. */
        const val NAME_SHRINK = 0.5f

        /** 뷰에 '앱별 칸인가'를 달아 두는 키. */
        const val ACCENT_KEY = 0x7f000001

        /** 조정 이유를 띄워 두는 시간. 데스크톱과 같다. */
        const val NOTICE_MS = 6000L

        /** 정지를 알리는 색. devup.json 의 light 테마 `$warning` 과 같은 값이다. */
        val WARNING: Int = Color.parseColor("#A85B00")
    }
}
