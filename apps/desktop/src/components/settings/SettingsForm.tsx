'use client'

import { Box, Flex, setTheme, Text, VStack } from '@devup-ui/react'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'

import { Choice } from '@/components/settings/Choice'
import { Range } from '@/components/settings/Range'
import { Section } from '@/components/settings/Section'
import { SwitchTester } from '@/components/settings/SwitchTester'
import { Toggle } from '@/components/settings/Toggle'
import { formatSeconds } from '@/lib/format'
import {
  closeSettings,
  getLogDirectory,
  type IntervalEvent,
  type Profile,
  saveProfile,
  type Theme,
  type UndoMapping,
} from '@/lib/profile'

/**
 * 설정 화면.
 *
 * 계층 메뉴를 만들지 않고 한 화면에 모두 펼친다. 저장 버튼도 두지 않는다 —
 * 스위치 하나로 조작하는 사용자에게는 '저장 버튼까지 이동해서 누르기'가
 * 설정을 바꾸는 것보다 비싼 일이다. 대신 바꾸는 즉시 반영·저장하고,
 * 되돌릴 수단으로 '기본값으로 되돌리기'를 둔다.
 */
export function SettingsForm({ initial }: { initial: Profile }) {
  const [draft, setDraft] = useState<Profile>(initial)
  const [warning, setWarning] = useState<string | null>(null)
  const [adjustment, setAdjustment] = useState<IntervalEvent | null>(null)
  const [logPath, setLogPath] = useState<string | null>(null)

  useEffect(() => {
    getLogDirectory()
      .then(setLogPath)
      .catch(() => {
        // 브라우저에서 화면만 확인할 때는 Tauri 컨텍스트가 없다.
      })
  }, [])

  // 드래그 중 매 프레임 저장하지 않도록 잠깐 모았다가 쓴다.
  useEffect(() => {
    const timer = setTimeout(() => {
      saveProfile(draft)
        .then((result) => {
          setWarning(result.warning)
          // 스위치 키 등록이 실패하면 코어가 이전 키로 되돌린다.
          // 화면이 그 사실을 반영하지 않으면 사용자는 바뀐 줄로 안다.
          if (result.warning) setDraft(result.profile)
        })
        .catch(() => {
          // 브라우저에서 화면만 확인할 때는 Tauri 컨텍스트가 없다.
        })
    }, 300)
    return () => clearTimeout(timer)
  }, [draft])

  useEffect(() => {
    setTheme(draft.theme)
  }, [draft.theme])

  useEffect(() => {
    const unlisten = listen<IntervalEvent>('scan://interval', (event) =>
      setAdjustment(event.payload),
    )
    return () => {
      unlisten.then((stop) => stop()).catch(() => {})
    }
  }, [])

  const update = (patch: Partial<Profile>) =>
    setDraft((previous) => ({ ...previous, ...patch }))

  return (
    <VStack bg="$background" gap="20px" minH="100vh" p="32px">
      <Flex alignItems="center" gap="16px" justifyContent="space-between">
        <Text color="$title" typography="h1">
          한번 설정
        </Text>
        <Box
          as="button"
          bg="$primary"
          borderRadius="12px"
          color="$base"
          cursor="pointer"
          onClick={() => {
            closeSettings().catch(() => {})
          }}
          px="24px"
          py="16px"
          typography="bodyL"
        >
          닫기
        </Box>
      </Flex>

      {warning && (
        <Box
          bg="$undoBg"
          borderColor="$undoText"
          borderRadius="12px"
          borderStyle="solid"
          borderWidth="2px"
          color="$undoText"
          p="16px"
        >
          <Text typography="bodyL">{warning}</Text>
        </Box>
      )}

      <Section
        description="커서가 다음 칸으로 넘어가는 간격입니다."
        title="주사 속도"
      >
        <Range
          label="현재 속도"
          max={draft.maxIntervalMs}
          min={draft.minIntervalMs}
          onChange={(intervalMs) => update({ intervalMs })}
          value={draft.intervalMs}
          valueText={formatSeconds(draft.intervalMs)}
        />
        {adjustment && (
          <Text color="$caption" typography="body">
            {adjustment.reason}
          </Text>
        )}
      </Section>

      <Section
        description="최근 반응을 보고 속도를 조금씩 맞춥니다. 아래 범위를 절대 벗어나지 않고, 수동 고정이 항상 우선합니다."
        title="적응 모드"
      >
        <Toggle
          checked={draft.adaptive}
          offLabel="적응 모드 꺼짐"
          onChange={(adaptive) => update({ adaptive })}
          onLabel="적응 모드 켜짐"
        />
        <Toggle
          checked={draft.manualLock}
          offLabel="수동 고정 꺼짐"
          onChange={(manualLock) => update({ manualLock })}
          onLabel="수동 고정 — 속도를 바꾸지 않음"
        />
        <Range
          label="가장 빠른 속도"
          max={4000}
          min={300}
          onChange={(minIntervalMs) =>
            update({
              minIntervalMs,
              maxIntervalMs: Math.max(minIntervalMs, draft.maxIntervalMs),
              intervalMs: Math.max(minIntervalMs, draft.intervalMs),
            })
          }
          value={draft.minIntervalMs}
          valueText={formatSeconds(draft.minIntervalMs)}
        />
        <Range
          label="가장 느린 속도"
          max={10000}
          min={300}
          onChange={(maxIntervalMs) =>
            update({
              maxIntervalMs,
              minIntervalMs: Math.min(maxIntervalMs, draft.minIntervalMs),
              intervalMs: Math.min(maxIntervalMs, draft.intervalMs),
            })
          }
          value={draft.maxIntervalMs}
          valueText={formatSeconds(draft.maxIntervalMs)}
        />
      </Section>

      <Section
        description="무엇을 언제 눌렀는지를 이 기기 안에 기록합니다. 실증에서 성공률과 되돌리기를 세는 데 씁니다."
        title="실증 기록"
      >
        <Toggle
          checked={draft.logging}
          offLabel="기록 꺼짐"
          onChange={(logging) => update({ logging })}
          onLabel="기록 켜짐"
        />
        <Text color="$caption" typography="body">
          기록은 <b>이 기기 밖으로 나가지 않습니다.</b> 켜고 끈 것은 다음
          실행부터 적용됩니다.
        </Text>
        {logPath && (
          <Text color="$caption" typography="caption">
            저장 위치: {logPath}
          </Text>
        )}
      </Section>

      <Section
        description="PDF 뷰어나 음악 앱을 쓰는 동안, 그 앱에서만 필요한 버튼을 뒤에 더 붙입니다. 앞의 네 칸은 어떤 앱에서도 자리가 바뀌지 않습니다."
        title="앱별 버튼"
      >
        <Toggle
          checked={draft.appButtons}
          offLabel="앱별 버튼 꺼짐 — 언제나 네 칸"
          onChange={(appButtons) => update({ appButtons })}
          onLabel="앱별 버튼 켜짐"
        />
        <Text color="$caption" typography="body">
          PDF 뷰어에서는 다음 장·이전 장이, 음악 앱에서는 재생/멈춤·다음 곡·이전
          곡이 붙습니다. 칸이 늘면 한 바퀴가 길어지므로, 쓰지 않는다면 꺼 두는
          편이 빠릅니다.
        </Text>
      </Section>

      <Section
        description="컨트롤러는 항상 맨 앞에 뜹니다. 지금 조작할 요소가 그 아래에 들어가면 창을 잠시 반투명하게 만들어 뒤가 보이게 합니다."
        title="가릴 때 반투명"
      >
        <Toggle
          checked={draft.dimWhenCovered}
          offLabel="반투명 꺼짐 — 항상 또렷하게"
          onChange={(dimWhenCovered) => update({ dimWhenCovered })}
          onLabel="반투명 켜짐"
        />
        <Range
          label="반투명 정도"
          max={100}
          min={25}
          onChange={(dimPercent) => update({ dimPercent })}
          step={5}
          value={draft.dimPercent}
          valueText={`${draft.dimPercent}%`}
        />
        <Text color="$caption" typography="body">
          숫자가 낮을수록 뒤가 잘 보이고 컨트롤러는 흐려집니다. 글자가 읽히지
          않으면 값을 올리세요. 되돌리기 창에서는 흐려지지 않습니다.
        </Text>
      </Section>

      <Section
        description="이보다 오래 누르면 '길게 누름'으로 읽어 취소하거나 일시정지합니다. 피로가 쌓여 누르는 시간이 길어지면 이 값을 올리세요."
        title="길게 누르기"
      >
        <Range
          label="길게 누름 기준"
          max={1500}
          min={300}
          onChange={(longPressMs) => update({ longPressMs })}
          step={50}
          value={draft.longPressMs}
          valueText={`${draft.longPressMs}밀리초`}
        />
        <SwitchTester longPressMs={draft.longPressMs} />
      </Section>

      <Section
        description="선택 직후 3초 안에 다시 누르면 이 동작을 보냅니다. 대상 프로그램이 이 단축키를 지원할 때만 실제로 되돌아갑니다."
        title="되돌리기 동작"
      >
        <Choice<UndoMapping>
          onChange={(undoMapping) => update({ undoMapping })}
          options={[
            { label: '뒤로 가기', value: 'back' },
            { label: '실행 취소', value: 'undo' },
          ]}
          value={draft.undoMapping}
        />
      </Section>

      <Section description="커서 이동과 선택을 소리로도 알립니다." title="소리">
        <Toggle
          checked={draft.sound}
          offLabel="소리 꺼짐"
          onChange={(sound) => update({ sound })}
          onLabel="소리 켜짐"
        />
      </Section>

      <Section description="화면이 잘 보이는 쪽을 고르세요." title="화면">
        <Choice<Theme>
          onChange={(theme) => update({ theme })}
          options={[
            { label: '밝게', value: 'light' },
            { label: '어둡게', value: 'dark' },
            { label: '고대비', value: 'contrast' },
          ]}
          value={draft.theme}
        />
      </Section>

      <Section
        description="스위치가 보내는 키입니다. 하드웨어를 바꿨을 때만 손대세요."
        title="스위치 키"
      >
        <Text color="$text" typography="bodyL">
          현재 키: {draft.switchKey}
        </Text>
      </Section>

      <Box
        as="button"
        bg="$scanIdleBg"
        borderColor="$borderBold"
        borderRadius="12px"
        borderStyle="solid"
        borderWidth="2px"
        color="$text"
        cursor="pointer"
        onClick={() =>
          update({
            intervalMs: 1800,
            minIntervalMs: 800,
            maxIntervalMs: 4000,
            adaptive: true,
            manualLock: false,
            longPressMs: 600,
            sound: true,
            undoMapping: 'back',
            dimWhenCovered: true,
            dimPercent: 40,
            appButtons: true,
            logging: true,
          })
        }
        px="24px"
        py="16px"
        typography="bodyL"
        w="fit-content"
      >
        기본값으로 되돌리기
      </Box>
    </VStack>
  )
}
