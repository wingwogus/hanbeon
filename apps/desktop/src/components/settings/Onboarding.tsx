'use client'

import { Box, Flex, Text, VStack } from '@devup-ui/react'
import { useState } from 'react'

import { Range } from '@/components/settings/Range'
import { Section } from '@/components/settings/Section'
import { SwitchTester } from '@/components/settings/SwitchTester'
import { formatSeconds } from '@/lib/format'
import { type Profile, saveProfile } from '@/lib/profile'

const STEPS = ['스위치 확인', '속도 맞추기', '저장'] as const

/**
 * 초기 설정 3단계.
 *
 * 목표는 신규 사용자가 10분 안에 마치는 것이다(PRD 정량 검증). 그래서
 * 단계마다 결정할 것을 하나로 줄이고, 나머지는 전부 기본값으로 둔 뒤
 * 나중에 설정 화면에서 손보게 한다.
 *
 * 이 화면 자체도 스위치로만 조작할 수 있어야 한다. 버튼은 Tab으로 닿을 수
 * 있는 실제 button이어야 하고, 한 화면에 몇 개 없어야 한다.
 */
export function Onboarding({
  initial,
  onDone,
}: {
  initial: Profile
  onDone: (profile: Profile) => void
}) {
  const [step, setStep] = useState(0)
  const [draft, setDraft] = useState<Profile>(initial)

  const update = (patch: Partial<Profile>) =>
    setDraft((previous) => ({ ...previous, ...patch }))

  const finish = async () => {
    const next = { ...draft, onboarded: true }
    try {
      const result = await saveProfile(next)
      onDone(result.profile)
    } catch {
      // 저장에 실패해도 안내를 처음부터 다시 시키지는 않는다.
      onDone(next)
    }
  }

  return (
    <VStack bg="$background" gap="24px" minH="100vh" p="32px">
      <VStack gap="8px">
        <Text color="$title" typography="h1">
          한번 시작하기
        </Text>
        <Flex gap="8px">
          {STEPS.map((name, index) => (
            <Text
              key={name}
              color={index === step ? '$primary' : '$caption'}
              typography="bodyL"
            >
              {/* 현재 단계를 색만으로 표시하지 않는다. */}
              {index === step ? '●' : '○'} {index + 1}. {name}
              {index < STEPS.length - 1 ? ' →' : ''}
            </Text>
          ))}
        </Flex>
      </VStack>

      {step === 0 && (
        <Section
          description={`스위치를 눌러 보세요. 아래에 반응이 나타나면 연결된 것입니다. 지금 설정된 키는 ${draft.switchKey}입니다.`}
          title="스위치가 연결됐는지 확인합니다"
        >
          <SwitchTester longPressMs={draft.longPressMs} />
        </Section>
      )}

      {step === 1 && (
        <Section
          description="커서가 칸을 옮겨 다니는 속도입니다. 편하게 누를 수 있는 정도로 맞추세요. 나중에 언제든 바꿀 수 있고, 적응 모드가 이 범위 안에서 조금씩 도와줍니다."
          title="속도를 맞춥니다"
        >
          <Range
            label="주사 속도"
            max={4000}
            min={600}
            onChange={(intervalMs) => update({ intervalMs })}
            value={draft.intervalMs}
            valueText={formatSeconds(draft.intervalMs)}
          />
        </Section>
      )}

      {step === 2 && (
        <Section
          description="이 설정으로 시작합니다. 설정 화면에서 언제든 다시 바꿀 수 있습니다."
          title="저장합니다"
        >
          <VStack gap="6px">
            <Text color="$text" typography="bodyL">
              주사 속도 {formatSeconds(draft.intervalMs)}
            </Text>
            <Text color="$text" typography="bodyL">
              길게 누름 기준 {draft.longPressMs}밀리초
            </Text>
            <Text color="$text" typography="bodyL">
              적응 모드 {draft.adaptive ? '켜짐' : '꺼짐'}
            </Text>
          </VStack>
        </Section>
      )}

      <Flex gap="12px">
        {step > 0 && (
          <Box
            as="button"
            bg="$scanIdleBg"
            borderColor="$borderBold"
            borderRadius="12px"
            borderStyle="solid"
            borderWidth="2px"
            color="$text"
            cursor="pointer"
            onClick={() => setStep((previous) => previous - 1)}
            px="28px"
            py="18px"
            typography="bodyL"
          >
            이전
          </Box>
        )}
        <Box
          as="button"
          bg="$primary"
          borderRadius="12px"
          color="$base"
          cursor="pointer"
          onClick={() => {
            if (step === STEPS.length - 1) {
              void finish()
              return
            }
            setStep((previous) => previous + 1)
          }}
          px="28px"
          py="18px"
          typography="bodyL"
        >
          {step === STEPS.length - 1 ? '저장하고 시작' : '다음'}
        </Box>
      </Flex>
    </VStack>
  )
}
