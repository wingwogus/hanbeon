import { VStack } from '@devup-ui/react'

export function ContentsContainer({ children }: { children: React.ReactNode }) {
  return (
    <VStack
      bg="$containerBackground"
      border="1px solid $border"
      borderRadius={[0, null, 2.5]}
      p={[4, null, 5]}
    >
      {children}
    </VStack>
  )
}
