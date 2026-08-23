import { Button } from '@devup-ui/components'
import { css, Text } from '@devup-ui/react'

export function HeaderButton({
  children,
  fit = false,
}: {
  children: React.ReactNode
  fit?: boolean
}) {
  return (
    <Button
      className={css({
        w: fit ? 'fit-content' : ['100%', null, 'fit-content'],
        px: 5,
        borderRadius: 1.5,
        py: [2, null, 2.5],
        border: '1px solid $borderBold',
      })}
    >
      <Text color="$text" typography="bodyS600" whiteSpace="nowrap">
        {children}
      </Text>
    </Button>
  )
}
