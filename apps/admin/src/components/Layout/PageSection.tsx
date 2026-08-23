import { Button } from '@devup-ui/components'
import { css, Flex, Text, VStack } from '@devup-ui/react'
import Link from 'next/link'

import { BackButton } from '../Buttons/BackButton'

export function PageSection({
  title,
  children,
  button = false,
  maxW = false,
  back = false,
  caption = '',
  buttonTo = 'edit',
}: {
  title: React.ReactNode
  children: React.ReactNode
  button?: boolean
  buttonTo?: string
  maxW?: boolean
  back?: boolean
  caption?: string
}) {
  return (
    <VStack
      gap={[3, null, 5]}
      maxW={maxW ? '1400px' : null}
      p={[0, null, '30px']}
    >
      <Flex justifyContent="space-between" pt={[3, null, 0]} px={[2, null, 0]}>
        <Flex alignItems={caption ? 'flex-start' : 'center'} gap="10px">
          {back && <BackButton />}
          <VStack>
            <Text color="$title" typography="h6">
              {title}
            </Text>
            <Text color="$caption" typography="bodyS">
              {caption}
            </Text>
          </VStack>
        </Flex>
        {button && (
          <Link href={buttonTo}>
            <Button className={css({ px: 5, py: '10px' })} variant="primary">
              <Text typography="bodyS600">신규 게시글 작성</Text>
            </Button>
          </Link>
        )}
      </Flex>
      {children}
    </VStack>
  )
}
