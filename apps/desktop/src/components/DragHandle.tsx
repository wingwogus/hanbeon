'use client'

import { Box, Center } from '@devup-ui/react'
import { getCurrentWindow } from '@tauri-apps/api/window'

/**
 * 창을 끌어 옮기는 손잡이.
 *
 * 창 전체를 잡히게 하지 않는다. 4칸 위 아무 데나 끌리면, 화면을 확인하려던
 * 손짓 한 번에 컨트롤러가 따라와 사용자가 익힌 위치가 매번 달라진다.
 *
 * 옮긴 위치는 코어가 프로필에 적고(`window::MoveWatch`), 이동이 멎으면 활성
 * 상태를 대상 앱에 돌려준다. 창을 끄는 순간 우리 앱이 활성 앱이 되는데,
 * 그대로 두면 이후 주입한 Tab이 대상 앱이 아니라 이 창으로 들어간다.
 */
export function DragHandle() {
  return (
    <Center
      aria-label="창 옮기기"
      cursor="grab"
      flexShrink={0}
      h="16px"
      onPointerDown={(event) => {
        // 왼쪽 버튼으로만 끈다. 오른쪽 버튼까지 받으면 컨텍스트 메뉴를
        // 열려던 조작이 창 이동으로 새어 나간다.
        if (event.button !== 0) return

        getCurrentWindow()
          .startDragging()
          .catch(() => {
            // 브라우저에서 화면만 확인할 때는 Tauri 컨텍스트가 없다.
          })
      }}
      w="100%"
    >
      <Box bg="$dragGrip" borderRadius="999px" h="4px" w="44px" />
    </Center>
  )
}
