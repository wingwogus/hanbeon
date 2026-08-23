import { Text, VStack } from '@devup-ui/react'

/**
 * 숫자 하나를 크게 보여주는 칸.
 *
 * 분포나 추이가 아니라 '지금 값이 얼마인가'만 알면 되는 지표는 차트로 그리지
 * 않는다. 막대 하나짜리 그래프는 숫자보다 읽기 어렵다.
 */
export function StatTile({
  label,
  value,
  hint,
}: {
  label: string
  value: string
  /** 값을 어떻게 읽어야 하는지. 숫자만으로 오해할 수 있는 지표에 단다. */
  hint?: string
}) {
  return (
    <VStack
      bg="$containerBackground"
      border="1px solid $border"
      borderRadius="10px"
      gap="6px"
      p="16px"
    >
      <Text color="$caption" typography="bodyS">
        {label}
      </Text>
      <Text
        color="$title"
        fontVariantNumeric="tabular-nums"
        typography="h6"
        wordBreak="keep-all"
      >
        {value}
      </Text>
      {hint && (
        <Text color="$caption" typography="bodyS" wordBreak="keep-all">
          {hint}
        </Text>
      )}
    </VStack>
  )
}
