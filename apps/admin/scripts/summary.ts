/**
 * 실증 기록을 읽어 지표를 찍는다.
 *
 *   bun run summary                     # OS 로그 폴더 전체
 *   bun run summary events-2026-08-23.jsonl
 *   bun run summary ~/Library/Logs/kr.devfive.hanbeon --json
 *
 * 계산은 `src/lib/metrics.ts`에 있다. 대시보드가 같은 함수를 쓰게 해서 화면의
 * 숫자와 현장에서 뽑은 숫자가 갈리지 않게 한다.
 */

import { readdirSync, readFileSync, statSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'

import {
  type Distribution,
  parseLog,
  splitSessions,
  summarize,
  type Summary,
} from '../src/lib/metrics'

/** 앱이 기록을 남기는 곳. 인자를 주지 않으면 여기를 본다. */
function defaultDirectory(): string {
  const id = 'kr.devfive.hanbeon'
  switch (process.platform) {
    case 'darwin':
      return join(homedir(), 'Library', 'Logs', id)
    case 'win32':
      return join(process.env.LOCALAPPDATA ?? homedir(), id, 'logs')
    default:
      return join(homedir(), '.local', 'share', id, 'logs')
  }
}

/** CLI의 출력 통로. `console`은 앱 코드에서 금지돼 있어 표준 스트림을 직접 쓴다. */
function out(text: string, error = false): void {
  const stream = error ? process.stderr : process.stdout
  stream.write(`${text}\n`)
}

function collect(paths: string[]): string[] {
  const files: string[] = []

  for (const path of paths) {
    let stat
    try {
      stat = statSync(path)
    } catch {
      out(`읽을 수 없습니다: ${path}`, true)
      continue
    }

    if (stat.isDirectory()) {
      files.push(
        ...readdirSync(path)
          .filter((name) => name.endsWith('.jsonl'))
          .map((name) => join(path, name)),
      )
    } else {
      files.push(path)
    }
  }

  return files.sort()
}

const seconds = (ms: number) => `${(ms / 1000).toFixed(1)}초`
const percent = (value: number | null) =>
  value === null ? '—' : `${(value * 100).toFixed(1)}%`

function spread(label: string, value: Distribution | null): string {
  if (!value) return `  ${label}: 표본 없음`
  return [
    `  ${label} (${value.count}회)`,
    `    평균 ${value.meanMs}ms · 중앙 ${value.medianMs}ms · 상위10% ${value.p90Ms}ms`,
    `    최소 ${value.minMs}ms · 최대 ${value.maxMs}ms`,
  ].join('\n')
}

function report(summary: Summary, index: number): string {
  const s = summary
  const lines: string[] = []

  lines.push(
    `── 세션 ${index + 1} ${'─'.repeat(40)}`,
    `  ${s.session.startedAt}  →  ${s.session.endedAt}`,
    `  기록 ${seconds(s.durationMs)}${s.session.closed ? '' : ' (정상 종료 기록 없음 — 강제 종료로 보임)'}`,
    '',
    `  선택 실행 ${s.actions}회${s.failedActions ? ` (주입 실패 ${s.failedActions}회)` : ''}`,
    `  되돌리기 ${s.undos}회 · 성공 ${percent(s.undos ? s.succeededUndos / s.undos : null)}`,
    `  되돌리기율 ${percent(s.undoRate)}   ← 오선택 대리 지표 (되돌리기 ÷ Enter 실행)`,
    `  놓침 ${s.missed}회 · ${percent(s.missRate)}   ← 원하는 칸을 지나쳐 한 바퀴 더 기다림`,
    '',
    spread('반응시간', s.reaction),
    spread('눌림 시간', s.held),
  )

  if (s.intervalStartMs !== null) {
    lines.push(
      '',
      `  주사 간격 ${s.intervalStartMs}ms → ${s.intervalEndMs}ms (${s.intervalChanges.length}회 조정)`,
    )
    for (const change of s.intervalChanges) {
      lines.push(`    ${change.reason}`)
    }
  }

  if (s.pausedMs > 0) lines.push('', `  정지 ${seconds(s.pausedMs)}`)

  const named = s.presets.filter((span) => span.preset)
  if (named.length > 0) {
    lines.push('', '  앱별 칸')
    for (const span of named) {
      lines.push(`    ${span.preset} (${span.cells}칸) — ${seconds(span.ms)}`)
    }
  }

  return lines.join('\n')
}

const args = process.argv.slice(2)
const asJson = args.includes('--json')
const targets = args.filter((arg) => !arg.startsWith('--'))

const files = collect(targets.length > 0 ? targets : [defaultDirectory()])

if (files.length === 0) {
  out(
    `기록 파일이 없습니다. 앱 설정의 '실증 기록'에서 저장 위치를 확인하세요.`,
    true,
  )
  process.exit(1)
}

const lines = files.flatMap((file) => parseLog(readFileSync(file, 'utf8')))
const summaries = splitSessions(lines).map(summarize)

if (asJson) {
  out(JSON.stringify(summaries, null, 2))
} else {
  out(
    `파일 ${files.length}개 · 줄 ${lines.length}개 · 세션 ${summaries.length}개\n`,
  )
  out(summaries.map(report).join('\n\n'))

  // 의도는 기록에 없다. 이 안내를 빼면 숫자를 성공률로 오해하게 된다.
  out(
    '\n※ 명령 선택 성공률은 여기서 셀 수 없습니다. 사용자가 무엇을 누르려 했는지는\n' +
      '   기록에 없으므로, 진행자가 과업 대본과 대조해 세야 합니다(PRD 10.1).',
  )
}
