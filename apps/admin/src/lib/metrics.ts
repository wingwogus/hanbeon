/**
 * 실증 기록을 지표로 접는다.
 *
 * 코어가 남긴 JSON Lines(PRD 10.1)를 읽어 세션별 숫자를 낸다. 대시보드와
 * 현장에서 쓰는 CLI가 **같은 함수**를 쓴다 — 계산이 두 벌이 되면 화면의 숫자와
 * 보고서의 숫자가 갈린다.
 *
 * 여기서 내는 것은 **기록으로 셀 수 있는 것뿐이다.** 사용자가 무엇을 누르려
 * 했는지는 코어가 알 수 없으므로 명령 선택 성공률은 진행자가 과업 대본과
 * 대조해 센다(PRD 10.1). 이 모듈은 그 대조에 필요한 원자료를 정리한다.
 */

/** 기록의 한 줄. 모르는 사건도 버리지 않고 그대로 들고 있는다. */
export interface LogLine {
  at: string
  ms: number
  event: string
  [key: string]: unknown
}

/** 한 번의 앱 실행. `session start`로 자른다. */
export interface Session {
  startedAt: string
  endedAt: string
  /** 실행이 정상 종료로 끝났는지. 강제 종료에서는 `session stop`이 남지 않는다. */
  closed: boolean
  lines: LogLine[]
}

export interface Summary {
  session: Session
  /** 실행 시간(밀리초). 첫 줄과 마지막 줄의 간격이다. */
  durationMs: number

  /** 칸을 실행한 횟수. */
  actions: number
  /** 그중 키 주입이 실패한 횟수. */
  failedActions: number
  /** 되돌리기 횟수와 성공 수. */
  undos: number
  succeededUndos: number

  /**
   * 되돌리기율 — 되돌리기 ÷ 선택(Enter) 실행.
   *
   * 오선택의 대리 지표다. 되돌렸다는 것은 직전 선택이 의도와 달랐다는 뜻이다.
   * 선택을 한 번도 하지 않았으면 `null`이다 — 0으로 적으면 '오선택이 없었다'로
   * 잘못 읽힌다.
   */
  undoRate: number | null

  /**
   * 놓침 — 원하는 칸을 지나쳐 한 바퀴를 더 기다린 횟수.
   *
   * 지나온 자리 수가 그때의 순환 자리 수 이상인 실행을 센다.
   */
  missed: number
  missRate: number | null

  /** 반응시간 분포(밀리초). 커서가 칸에 들어온 뒤 누르기까지. */
  reaction: Distribution | null

  /** 눌림 시간 분포(밀리초). 짧게/길게 임계값을 맞추는 근거다. */
  held: Distribution | null

  /** 주사 간격이 바뀐 이력. */
  intervalChanges: { fromMs: number; toMs: number; reason: string }[]
  /** 처음과 마지막 주사 간격. */
  intervalStartMs: number | null
  intervalEndMs: number | null

  /** 정지해 있던 시간의 합(밀리초). */
  pausedMs: number

  /** 앱별 칸이 붙어 있던 구간. */
  presets: { preset: string | null; cells: number; ms: number }[]
}

export interface Distribution {
  count: number
  meanMs: number
  medianMs: number
  p90Ms: number
  minMs: number
  maxMs: number
}

/** 한 줄씩 읽는다. 깨진 줄은 버리고 나머지를 살린다. */
export function parseLog(text: string): LogLine[] {
  const lines: LogLine[] = []

  for (const raw of text.split('\n')) {
    const trimmed = raw.trim()
    if (!trimmed) continue

    try {
      const value = JSON.parse(trimmed)
      // 앱이 쓰는 도중에 죽으면 마지막 줄이 잘려 있을 수 있다. 그 한 줄
      // 때문에 앞의 기록을 통째로 버리지 않는다.
      if (typeof value?.event === 'string' && typeof value?.ms === 'number') {
        lines.push(value as LogLine)
      }
    } catch {
      // 읽지 못한 줄은 건너뛴다.
    }
  }

  return lines.sort((a, b) => a.ms - b.ms)
}

/** `session start`를 경계로 실행 단위를 나눈다. */
export function splitSessions(lines: LogLine[]): Session[] {
  const sessions: Session[] = []
  let current: LogLine[] = []

  const close = () => {
    if (current.length === 0) return
    const first = current[0]
    const last = current.at(-1) as LogLine
    sessions.push({
      startedAt: first.at,
      endedAt: last.at,
      closed: current.some(
        (line) => line.event === 'session' && line.phase === 'stop',
      ),
      lines: current,
    })
    current = []
  }

  for (const line of lines) {
    if (line.event === 'session' && line.phase === 'start') close()
    current.push(line)
  }
  close()

  return sessions
}

function distribution(values: number[]): Distribution | null {
  if (values.length === 0) return null

  const sorted = [...values].sort((a, b) => a - b)
  const at = (ratio: number) =>
    sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * ratio))]

  return {
    count: sorted.length,
    meanMs: Math.round(sorted.reduce((sum, v) => sum + v, 0) / sorted.length),
    medianMs: at(0.5),
    p90Ms: at(0.9),
    minMs: sorted[0],
    maxMs: sorted[sorted.length - 1],
  }
}

function ratio(part: number, whole: number): number | null {
  return whole === 0 ? null : part / whole
}

/** 한 세션을 지표로 접는다. */
export function summarize(session: Session): Summary {
  const { lines } = session

  const actions = lines.filter((line) => line.event === 'action')
  const undos = lines.filter((line) => line.event === 'undo')
  const enters = actions.filter((line) => line.action === 'enter')

  // 놓침은 '지나온 자리 수 ≥ 그때의 순환 자리 수'다. 순환 자리 수는 앱에 따라
  // 달라져서, 기록에 함께 남긴 값을 그대로 쓴다.
  const missed = actions.filter(
    (line) =>
      typeof line.steps === 'number' &&
      typeof line.cycle === 'number' &&
      line.cycle > 0 &&
      line.steps >= line.cycle,
  ).length

  const intervalChanges = lines
    .filter((line) => line.event === 'interval')
    .map((line) => ({
      fromMs: Number(line.fromMs),
      toMs: Number(line.toMs),
      reason: String(line.reason ?? ''),
    }))

  const cursors = lines.filter((line) => line.event === 'cursor')

  return {
    session,
    durationMs: (lines.at(-1)?.ms ?? 0) - (lines[0]?.ms ?? 0),

    actions: actions.length,
    failedActions: actions.filter((line) => line.ok === false).length,
    undos: undos.length,
    succeededUndos: undos.filter((line) => line.ok === true).length,
    undoRate: ratio(undos.length, enters.length),

    missed,
    missRate: ratio(missed, actions.length),

    reaction: distribution(
      actions
        .map((line) => Number(line.reactionMs))
        .filter((value) => Number.isFinite(value)),
    ),
    held: distribution(
      lines
        .filter((line) => line.event === 'input')
        .map((line) => Number(line.heldMs))
        .filter((value) => Number.isFinite(value)),
    ),

    intervalChanges,
    intervalStartMs: cursors.length ? Number(cursors[0].intervalMs) : null,
    intervalEndMs: cursors.length
      ? Number((cursors.at(-1) as LogLine).intervalMs)
      : null,

    pausedMs: pausedDuration(lines),
    presets: presetSpans(lines),
  }
}

/**
 * 정지해 있던 시간의 합.
 *
 * 정지한 채로 앱이 끝나면 마지막 구간은 기록의 끝까지로 본다. 열린 채 버리면
 * '오래 멈춰 있었다'는 사실이 통째로 사라진다.
 */
function pausedDuration(lines: LogLine[]): number {
  let total = 0
  let since: number | null = null

  for (const line of lines) {
    if (line.event !== 'pause') continue
    if (line.paused === true && since === null) since = line.ms
    else if (line.paused === false && since !== null) {
      total += line.ms - since
      since = null
    }
  }

  if (since !== null) total += (lines.at(-1)?.ms ?? since) - since
  return total
}

/** 어떤 앱별 칸이 얼마나 붙어 있었는지. */
function presetSpans(lines: LogLine[]): Summary['presets'] {
  const spans: Summary['presets'] = []
  let open: { preset: string | null; cells: number; ms: number } | null = null

  for (const line of lines) {
    if (line.event !== 'preset') continue

    if (open) {
      spans.push({ ...open, ms: line.ms - open.ms })
    }
    open = {
      preset: (line.preset as string | null) ?? null,
      cells: Number(line.cells),
      ms: line.ms,
    }
  }

  if (open) {
    spans.push({ ...open, ms: (lines.at(-1)?.ms ?? open.ms) - open.ms })
  }

  return spans
}

/** 분포를 막대로 그릴 수 있게 구간별로 센다. */
export interface Bin {
  /** 구간의 시작(밀리초). */
  fromMs: number
  /** 구간의 끝. 마지막 구간은 열려 있어 `null`이다. */
  toMs: number | null
  count: number
}

/**
 * 값을 고정 폭 구간으로 센다.
 *
 * 마지막 구간은 열어 둔다. 반응시간은 위쪽으로 길게 늘어지는데, 상한을 두고
 * 자르면 '아주 오래 걸린 경우'가 통째로 사라져 정작 봐야 할 꼬리를 잃는다.
 */
export function histogram(values: number[], binMs: number, bins = 8): Bin[] {
  if (values.length === 0 || binMs <= 0 || bins <= 0) return []

  const result: Bin[] = Array.from({ length: bins }, (_, index) => ({
    fromMs: index * binMs,
    toMs: index === bins - 1 ? null : (index + 1) * binMs,
    count: 0,
  }))

  for (const value of values) {
    const index = Math.min(bins - 1, Math.max(0, Math.floor(value / binMs)))
    result[index].count += 1
  }

  return result
}

/** 세션 안에서 주사 간격이 어떻게 움직였는지, 시작부터의 경과 시간과 함께. */
export interface IntervalPoint {
  /** 세션 시작부터의 경과(밀리초). */
  atMs: number
  intervalMs: number
  reason: string | null
}

export function intervalTrack(session: Session): IntervalPoint[] {
  const start = session.lines[0]?.ms ?? 0
  const first = session.lines.find((line) => line.event === 'cursor')
  if (!first) return []

  const points: IntervalPoint[] = [
    {
      atMs: first.ms - start,
      intervalMs: Number(first.intervalMs),
      reason: null,
    },
  ]

  for (const line of session.lines) {
    if (line.event !== 'interval') continue
    points.push({
      atMs: line.ms - start,
      intervalMs: Number(line.toMs),
      reason: String(line.reason ?? ''),
    })
  }

  // 마지막 값이 세션 끝까지 이어졌음을 보이려면 끝점이 하나 더 필요하다.
  const last = session.lines.at(-1)
  const tail = points.at(-1)
  if (last && tail && last.ms - start > tail.atMs) {
    points.push({
      atMs: last.ms - start,
      intervalMs: tail.intervalMs,
      reason: null,
    })
  }

  return points
}
