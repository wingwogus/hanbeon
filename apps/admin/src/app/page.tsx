import { Text, VStack } from '@devup-ui/react'

import { LogReport } from '@/components/Report/LogReport'

export default function Page() {
  return (
    <VStack gap="20px" maxW="1400px" mx="auto" p={[2, null, '30px']}>
      <VStack gap="4px">
        <Text color="$title" typography="h6">
          실증 기록
        </Text>
        <Text color="$caption" typography="bodyS">
          데스크톱 앱이 남긴 기록을 읽어 실증 지표를 봅니다. 파일은 브라우저
          안에서만 열리고 서버로 올라가지 않습니다.
        </Text>
      </VStack>
      <LogReport />
    </VStack>
  )
}
