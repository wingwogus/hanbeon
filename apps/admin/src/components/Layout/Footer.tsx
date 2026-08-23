import { Text } from '@devup-ui/react'

export function Footer() {
  return (
    <Text
      color="$caption"
      opacity="0.7"
      p={[4, null, 5]}
      typography="caption"
      wordBreak="keep-all"
    >
      한번(HanBeon) 실증 대시보드 · MIT License · 제7회 국립재활원 보조기기
      해커톤
    </Text>
  )
}
