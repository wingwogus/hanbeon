import { describe, expect, it } from 'bun:test'

import {
  histogram,
  intervalTrack,
  parseLog,
  splitSessions,
  summarize,
} from '@/lib/metrics'

/** 기록 한 줄을 만든다. 시각은 밀리초만 맞으면 된다. */
const line = (ms: number, event: string, rest: object = {}) =>
  JSON.stringify({
    at: new Date(ms).toISOString(),
    ms,
    event,
    ...rest,
  })

const log = (...lines: string[]) => lines.join('\n')

describe('parseLog', () => {
  it('깨진 줄이 있어도 나머지를 살린다', () => {
    // 앱이 쓰는 도중에 죽으면 마지막 줄이 잘려 있을 수 있다. 그 한 줄 때문에
    // 앞의 기록을 통째로 버리면 실증 자료를 잃는다.
    const parsed = parseLog(
      log(
        line(1, 'session', { phase: 'start' }),
        '{"event":"action","ms":2,',
        line(3, 'input', { gesture: 'short', heldMs: 100 }),
      ),
    )

    expect(parsed).toHaveLength(2)
    expect(parsed.map((l) => l.event)).toEqual(['session', 'input'])
  })

  it('시간 순으로 정렬한다', () => {
    const parsed = parseLog(log(line(30, 'input'), line(10, 'cursor')))
    expect(parsed.map((l) => l.ms)).toEqual([10, 30])
  })
})

describe('splitSessions', () => {
  it('실행 시작을 경계로 자른다', () => {
    const sessions = splitSessions(
      parseLog(
        log(
          line(1, 'session', { phase: 'start' }),
          line(2, 'input'),
          line(10, 'session', { phase: 'start' }),
          line(11, 'input'),
        ),
      ),
    )

    expect(sessions).toHaveLength(2)
    expect(sessions[0].lines).toHaveLength(2)
  })

  it('정상 종료 여부를 구분한다', () => {
    // 강제 종료에서는 stop이 남지 않는다. 이걸 구분하지 못하면 실증 구간이
    // 어디서 끊겼는지 알 수 없다.
    const [forced] = splitSessions(
      parseLog(log(line(1, 'session', { phase: 'start' }), line(2, 'input'))),
    )
    const [closed] = splitSessions(
      parseLog(
        log(
          line(1, 'session', { phase: 'start' }),
          line(2, 'session', { phase: 'stop' }),
        ),
      ),
    )

    expect(forced.closed).toBe(false)
    expect(closed.closed).toBe(true)
  })
})

describe('summarize', () => {
  const session = () =>
    splitSessions(
      parseLog(
        log(
          line(0, 'session', { phase: 'start' }),
          line(100, 'input', { gesture: 'short', heldMs: 120 }),
          line(110, 'action', {
            cell: '>',
            action: 'next',
            reactionMs: 400,
            steps: 1,
            cycle: 5,
            ok: true,
          }),
          line(200, 'input', { gesture: 'short', heldMs: 180 }),
          line(210, 'action', {
            cell: 'Enter',
            action: 'enter',
            reactionMs: 800,
            steps: 6,
            cycle: 5,
            ok: true,
          }),
          line(300, 'undo', { mapping: 'back', ok: true }),
          line(400, 'interval', {
            fromMs: 1800,
            toMs: 2200,
            reason: '실수가 감지되어 1.8초 → 2.2초',
          }),
          line(500, 'pause', { paused: true }),
          line(1500, 'pause', { paused: false }),
          line(2000, 'cursor', {
            cursor: 0,
            cell: '>',
            mode: 'scanning',
            intervalMs: 2200,
          }),
        ),
      ),
    )[0]

  it('놓침은 지나온 자리가 순환 자리 수 이상인 실행이다', () => {
    // 두 번째 실행만 6 ≥ 5 라서 놓침이다.
    expect(summarize(session()).missed).toBe(1)
  })

  it('되돌리기율은 선택 실행을 분모로 쓴다', () => {
    // 되돌리기 1회 ÷ Enter 실행 1회.
    expect(summarize(session()).undoRate).toBe(1)
  })

  it('선택이 없으면 되돌리기율은 0이 아니라 없음이다', () => {
    // 0으로 적으면 '오선택이 없었다'로 잘못 읽힌다.
    const empty = splitSessions(
      parseLog(log(line(0, 'session', { phase: 'start' }), line(1, 'cursor'))),
    )[0]

    expect(summarize(empty).undoRate).toBeNull()
  })

  it('정지해 있던 시간을 더한다', () => {
    expect(summarize(session()).pausedMs).toBe(1000)
  })

  it('정지한 채 끝나면 기록 끝까지를 정지로 본다', () => {
    // 열린 구간을 버리면 오래 멈춰 있었다는 사실이 통째로 사라진다.
    const stuck = splitSessions(
      parseLog(
        log(
          line(0, 'session', { phase: 'start' }),
          line(100, 'pause', { paused: true }),
          line(600, 'cursor'),
        ),
      ),
    )[0]

    expect(summarize(stuck).pausedMs).toBe(500)
  })

  it('반응시간 분포를 낸다', () => {
    const reaction = summarize(session()).reaction
    expect(reaction?.count).toBe(2)
    expect(reaction?.meanMs).toBe(600)
    expect(reaction?.maxMs).toBe(800)
  })

  it('앱별 칸이 붙어 있던 구간을 잰다', () => {
    // 어느 앱에서 얼마나 썼는지를 알아야, 앱별 칸이 실제로 값을 했는지 본다.
    const spans = summarize(
      splitSessions(
        parseLog(
          log(
            line(0, 'session', { phase: 'start' }),
            line(1000, 'preset', { preset: 'PDF 뷰어', cells: 6 }),
            line(4000, 'preset', { preset: null, cells: 4 }),
            line(9000, 'cursor'),
          ),
        ),
      )[0],
    ).presets

    expect(spans).toHaveLength(2)
    expect(spans[0]).toMatchObject({ preset: 'PDF 뷰어', cells: 6, ms: 3000 })
    // 마지막 구간은 기록 끝까지로 본다.
    expect(spans[1]).toMatchObject({ preset: null, ms: 5000 })
  })

  it('간격 변경을 이유까지 남긴다', () => {
    const changes = summarize(session()).intervalChanges
    expect(changes).toHaveLength(1)
    expect(changes[0].toMs).toBe(2200)
    expect(changes[0].reason).toContain('실수가 감지되어')
  })
})

describe('histogram', () => {
  it('구간마다 값을 센다', () => {
    const bins = histogram([100, 900, 1200, 1800], 1000, 3)
    expect(bins.map((bin) => bin.count)).toEqual([2, 2, 0])
  })

  it('마지막 구간은 열어 둔다', () => {
    // 반응시간은 위쪽으로 길게 늘어진다. 상한에서 잘라 버리면 정작 봐야 할
    // 꼬리가 통째로 사라진다.
    const bins = histogram([100, 99_000], 1000, 3)
    expect(bins.at(-1)).toMatchObject({ toMs: null, count: 1 })
  })

  it('표본이 없으면 빈 배열이다', () => {
    expect(histogram([], 1000)).toEqual([])
  })
})

describe('intervalTrack', () => {
  const session = (...lines: string[]) =>
    splitSessions(
      parseLog(log(line(0, 'session', { phase: 'start' }), ...lines)),
    )[0]

  it('첫 간격에서 시작해 조정마다 점을 찍는다', () => {
    const points = intervalTrack(
      session(
        line(100, 'cursor', { intervalMs: 1800 }),
        line(500, 'interval', { fromMs: 1800, toMs: 2200, reason: '실수' }),
        line(900, 'cursor', { intervalMs: 2200 }),
      ),
    )

    expect(points.map((p) => p.intervalMs)).toEqual([1800, 2200, 2200])
    expect(points[1].reason).toBe('실수')
  })

  it('마지막 값이 세션 끝까지 이어진 것으로 본다', () => {
    // 끝점이 없으면 마지막 구간이 그려지지 않아, 조정 직후에 세션이 끝난
    // 것처럼 보인다.
    const points = intervalTrack(
      session(line(100, 'cursor', { intervalMs: 1800 }), line(5000, 'input')),
    )

    expect(points.at(-1)).toMatchObject({ atMs: 5000, intervalMs: 1800 })
  })

  it('커서 기록이 없으면 그릴 것이 없다', () => {
    expect(intervalTrack(session(line(10, 'input')))).toEqual([])
  })
})
