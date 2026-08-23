import { Button } from '@devup-ui/components'
import { Center, Text, VStack } from '@devup-ui/react'

import { Label } from '../../components/Forms/Label'

export default function SignInPage() {
  return (
    <Center bg="$background" h="100vh">
      <VStack
        alignItems="center"
        bg="$containerBackground"
        border="1px solid $borderBold"
        borderRadius={4}
        gap="50px"
        maxW="500px"
        pb={20}
        pt="50px"
        px="20px"
        w="100%"
      >
        <Text color="$title" typography="h5">
          관리 페이지 로그인
        </Text>
        <VStack gap={5} maxW="300px" w="100%">
          <Label label="아이디">
            <input placeholder="아이디를 입력해주세요." />
          </Label>
          <Label label="비밀번호">
            <input placeholder="비밀번호를 입력해주세요." />
          </Label>

          <Text color="$error" typography="bodyS">
            에러 메시지
          </Text>

          <Button variant="primary">
            <Text typography="body700">로그인</Text>
          </Button>
        </VStack>
      </VStack>
    </Center>
  )
}
