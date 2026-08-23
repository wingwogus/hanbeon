import { Footer } from '@components/Layout/Footer'
import { Header } from '@components/Layout/Header'
import { SideBar } from '@components/SideMenu/SideBar'
import { Box, Flex, VStack } from '@devup-ui/react'

export default function Layout({ children }: { children: React.ReactNode }) {
  return (
    <VStack>
      <Header />
      <Flex>
        <SideBar />
        <VStack
          bg="$background"
          flex="1"
          minH="100dvh"
          pt={['54px', null, '60px']}
        >
          <Box flex={1}>{children}</Box>
          <Footer />
        </VStack>
      </Flex>
    </VStack>
  )
}
