'use client'

import { Box, Flex, Grid, Text, VStack } from '@devup-ui/react'
import { useMemo, useState } from 'react'

import { Histogram } from '@/components/Report/Histogram'
import { IntervalChart } from '@/components/Report/IntervalChart'
import { StatTile } from '@/components/Report/StatTile'
import {
  histogram,
  intervalTrack,
  parseLog,
  splitSessions,
  summarize,
  type Summary,
} from '@/lib/metrics'

const seconds = (ms: number) => `${(ms / 1000).toFixed(1)}초`
const percent = (value: number | null) =>
  value === null ? '—' : `${(value * 100).toFixed(1)}%`
const clock = (iso: string) =>
  new Date(iso).toLocaleString('ko-KR', {
    dateStyle: 'short',
    timeStyle: 'medium',
  })

/**
 * 실증 기록을 읽어 보여준다.
 *
 * 파일은 **브라우저 안에서만** 읽는다. 서버로 올리지 않는다 — 실증 참여자의
 * 동의 없이 기록을 밖으로 내보내지 않는다는 것이 PRD 10.1의 약속이고,
 * 대시보드라고 예외일 이유가 없다.
 */
export function LogReport() {
  const [text, setText] = useState('')
  const [names, setNames] = useState<string[]>([])
  const [picked, setPicked] = useState(0)

  const summaries = useMemo<Summary[]>(
    () => (text ? splitSessions(parseLog(text)).map(summarize) : []),
    [text],
  )

  const current = summaries[Math.min(picked, summaries.length - 1)]

  async function read(files: FileList | null) {
    if (!files || files.length === 0) return

    const list = Array.from(files)
    const contents = await Promise.all(list.map((file) => file.text()))

    setNames(list.map((file) => file.name))
    setText(contents.join('\n'))
    setPicked(0)
  }

  return (
    <VStack gap="20px">
      <VStack
        bg="$containerBackground"
        border="1px dashed $borderBold"
        borderRadius="12px"
        gap="10px"
        p="20px"
      >
        <Text color="$title" typography="bodyL700">
          기록 파일 열기
        </Text>
        <Text color="$caption" typography="bodyS">
          데스크톱 앱 설정의 &lsquo;실증 기록&rsquo;에 적힌 폴더에서{' '}
          <Box as="span" color="$text">
            events-YYYY-MM-DD.jsonl
          </Box>
          을 고르세요. 여러 개를 한꺼번에 고를 수 있습니다.
        </Text>

        <Box
          accept=".jsonl,application/json"
          as="input"
          color="$text"
          multiple
          onChange={(event: React.ChangeEvent<HTMLInputElement>) =>
            read(event.target.files)
          }
          type="file"
          typography="bodyS"
        />

        <Text color="$caption" typography="bodyS">
          파일은 이 브라우저 안에서만 읽습니다. 서버로 올리지 않습니다.
        </Text>

        {names.length > 0 && (
          <Text color="$caption" typography="bodyS">
            불러온 파일: {names.join(', ')}
          </Text>
        )}
      </VStack>

      {summaries.length === 0 ? (
        <Text color="$caption" typography="body">
          아직 불러온 기록이 없습니다.
        </Text>
      ) : (
        <VStack gap="20px">
          <VStack gap="8px">
            <Text color="$title" typography="bodyL700">
              세션 {summaries.length}개
            </Text>
            <Flex flexWrap="wrap" gap="8px">
              {summaries.map((summary, index) => (
                <Box
                  key={summary.session.startedAt}
                  as="button"
                  bg={index === picked ? '$primary' : '$containerBackground'}
                  border="1px solid $border"
                  borderRadius="8px"
                  color={index === picked ? '$base' : '$text'}
                  cursor="pointer"
                  onClick={() => setPicked(index)}
                  px="12px"
                  py="8px"
                  typography="bodyS"
                >
                  {clock(summary.session.startedAt)} ·{' '}
                  {seconds(summary.durationMs)}
                </Box>
              ))}
            </Flex>
          </VStack>

          {current && <SessionView summary={current} />}
        </VStack>
      )}
    </VStack>
  )
}

function SessionView({ summary }: { summary: Summary }) {
  const reactions = summary.session.lines
    .filter((line) => line.event === 'action')
    .map((line) => Number(line.reactionMs))
    .filter((value) => Number.isFinite(value))

  return (
    <VStack gap="20px">
      {!summary.session.closed && (
        <Box
          bg="$containerBackground"
          border="1px solid $warning"
          borderRadius="8px"
          px="12px"
          py="8px"
        >
          <Text color="$warning" typography="bodyS">
            정상 종료 기록이 없습니다. 강제로 꺼진 세션으로 보입니다.
          </Text>
        </Box>
      )}

      <Grid
        gap="12px"
        gridTemplateColumns={['1fr 1fr', null, 'repeat(4, 1fr)']}
      >
        <StatTile
          hint={
            summary.failedActions > 0
              ? `주입 실패 ${summary.failedActions}회`
              : undefined
          }
          label="선택 실행"
          value={`${summary.actions}회`}
        />
        <StatTile
          hint="되돌리기 ÷ Enter 실행. 오선택 대리 지표"
          label="되돌리기율"
          value={percent(summary.undoRate)}
        />
        <StatTile
          hint={`${summary.missed}회 · 원하는 칸을 지나침`}
          label="놓침"
          value={percent(summary.missRate)}
        />
        <StatTile
          hint={
            summary.reaction
              ? `상위 10%는 ${summary.reaction.p90Ms}ms`
              : undefined
          }
          label="반응시간 중앙값"
          value={summary.reaction ? `${summary.reaction.medianMs}ms` : '—'}
        />
      </Grid>

      <Box
        bg="$containerBackground"
        border="1px solid $border"
        borderRadius="12px"
        p="20px"
      >
        <Histogram
          bins={histogram(reactions, 1000)}
          caption="커서가 칸에 들어온 뒤 스위치를 누르기까지. 적응 로직은 이 값의 평균에 300ms를 더한 값을 목표로 삼습니다."
          title="반응시간 분포"
        />
      </Box>

      <Box
        bg="$containerBackground"
        border="1px solid $border"
        borderRadius="12px"
        p="20px"
      >
        <IntervalChart
          caption="점은 적응 로직이 간격을 바꾼 순간입니다. 사용자가 정한 최소·최대를 벗어나지 않습니다."
          points={intervalTrack(summary.session)}
          title="주사 간격 변화"
        />
      </Box>

      {(summary.pausedMs > 0 || summary.presets.some((s) => s.preset)) && (
        <Flex flexWrap="wrap" gap="12px">
          {summary.pausedMs > 0 && (
            <StatTile
              hint="길게 눌러 멈춰 있던 시간"
              label="정지"
              value={seconds(summary.pausedMs)}
            />
          )}
          {summary.presets
            .filter((span) => span.preset)
            .map((span) => (
              <StatTile
                key={`${span.preset}-${span.ms}`}
                hint="앱별 칸이 붙어 있던 시간"
                label={`${span.preset} (${span.cells}칸)`}
                value={seconds(span.ms)}
              />
            ))}
        </Flex>
      )}

      {/* 나온 숫자를 성공률로 오해하지 않게 한다. 의도는 기록에 없다. */}
      <Text color="$caption" typography="bodyS">
        명령 선택 성공률은 여기서 셀 수 없습니다. 사용자가 무엇을 누르려
        했는지는 기록에 없으므로, 진행자가 과업 대본과 대조해 세야 합니다(PRD
        10.1).
      </Text>
    </VStack>
  )
}
