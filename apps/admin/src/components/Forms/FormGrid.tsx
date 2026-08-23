import { Grid } from '@devup-ui/react'

export function FormGrid({ children }: { children: React.ReactNode }) {
  return (
    <Grid
      columnGap={[6, null, 10]}
      gridTemplateColumns={['repeat(1, 1fr)', null, 'repeat(2, 1fr)']}
      p={[0, null, 2]}
      py={2}
      rowGap={6}
    >
      {children}
    </Grid>
  )
}
