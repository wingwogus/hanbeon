'use client'

import { Box, css, Text, VStack } from '@devup-ui/react'
import { useState } from 'react'

import type { Bin } from '@/lib/metrics'

/** 그리는 좌표계. 실제 크기는 뷰박스가 늘려 준다. */
const W = 640
const H = 200
const PAD = { top: 16, right: 8, bottom: 28, left: 8 }
/** 막대 사이 간격. 붙여 두면 두 막대가 한 덩어리로 읽힌다. */
const GAP = 2

/**
 * 위쪽 모서리만 둥근 막대.
 *
 * 네 모서리를 다 둥글리면 막대가 바닥선에서 떠 보인다. 값이 시작되는 곳은
 * 바닥이고, 거기서 떨어지면 길이를 눈으로 재기 어려워진다.
 */
function barPath(x: number, y: number, w: number, h: number): string {
  const r = Math.min(4, w / 2, h)
  return [
    `M ${x} ${y + r}`,
    `Q ${x} ${y} ${x + r} ${y}`,
    `L ${x + w - r} ${y}`,
    `Q ${x + w} ${y} ${x + w} ${y + r}`,
    `L ${x + w} ${y + h}`,
    `L ${x} ${y + h}`,
    'Z',
  ].join(' ')
}

const label = (bin: Bin) =>
  bin.toMs === null
    ? `${bin.fromMs / 1000}초+`
    : `${bin.fromMs / 1000}–${bin.toMs / 1000}초`

/**
 * 분포 막대.
 *
 * 계열이 하나뿐이라 범례를 두지 않는다 — 제목이 그것을 말한다. 모든 막대에
 * 숫자를 얹지 않고 가장 높은 막대에만 단다. 나머지는 가리키면 나온다.
 */
export function Histogram({
  bins,
  title,
  caption,
}: {
  bins: Bin[]
  title: string
  caption?: string
}) {
  const [hovered, setHovered] = useState<number | null>(null)

  const max = Math.max(1, ...bins.map((bin) => bin.count))
  const tallest = bins.findIndex((bin) => bin.count === max)

  const plotW = W - PAD.left - PAD.right
  const plotH = H - PAD.top - PAD.bottom
  const slot = plotW / Math.max(1, bins.length)

  return (
    <VStack gap="8px">
      <VStack gap="2px">
        <Text color="$title" typography="bodyL700">
          {title}
        </Text>
        {caption && (
          <Text color="$caption" typography="bodyS">
            {caption}
          </Text>
        )}
      </VStack>

      <Box pos="relative">
        <svg
          aria-label={title}
          // 인라인 svg에 이름을 붙이는 표준 방법이다. 규칙이 제안하는 <img>는
          // 외부 파일을 부를 때의 이야기라 여기엔 맞지 않는다.
          // eslint-disable-next-line jsx-a11y/prefer-tag-over-role
          role="img"
          viewBox={`0 0 ${W} ${H}`}
          width="100%"
        >
          {/* 바닥선만 둔다. 격자를 촘촘히 깔면 막대보다 눈에 먼저 들어온다. */}
          <line
            className={css({ stroke: '$border' })}
            strokeWidth={1}
            x1={PAD.left}
            x2={W - PAD.right}
            y1={PAD.top + plotH}
            y2={PAD.top + plotH}
          />

          {bins.map((bin, index) => {
            const height = (bin.count / max) * plotH
            const x = PAD.left + index * slot
            const y = PAD.top + plotH - height

            return (
              <g
                key={bin.fromMs}
                onMouseEnter={() => setHovered(index)}
                onMouseLeave={() => setHovered(null)}
              >
                {/* 실제 막대보다 넓은 판을 깔아 가리키기 쉽게 한다. */}
                <rect
                  fill="transparent"
                  height={plotH + PAD.top}
                  width={slot}
                  x={x}
                  y={0}
                />
                {bin.count > 0 && (
                  <path
                    className={css({ fill: '$chartAccent' })}
                    d={barPath(x + GAP / 2, y, slot - GAP, Math.max(2, height))}
                    opacity={hovered === null || hovered === index ? 1 : 0.55}
                  />
                )}
                {index === tallest && bin.count > 0 && (
                  <text
                    className={css({ fill: '$text' })}
                    fontSize={12}
                    textAnchor="middle"
                    x={x + slot / 2}
                    y={y - 5}
                  >
                    {bin.count}
                  </text>
                )}
                <text
                  className={css({ fill: '$caption' })}
                  fontSize={11}
                  textAnchor="middle"
                  x={x + slot / 2}
                  y={H - 10}
                >
                  {label(bin)}
                </text>
              </g>
            )
          })}
        </svg>

        {hovered !== null && bins[hovered] && (
          <Box
            bg="$containerBackground"
            border="1px solid $border"
            borderRadius="8px"
            boxShadow="0 2px 8px rgba(0,0,0,0.12)"
            left={`${((hovered + 0.5) / bins.length) * 100}%`}
            pointerEvents="none"
            pos="absolute"
            px="10px"
            py="6px"
            top="0"
            transform="translateX(-50%)"
          >
            <Text color="$text" typography="bodyS" whiteSpace="nowrap">
              {label(bins[hovered])} · {bins[hovered].count}회
            </Text>
          </Box>
        )}
      </Box>
    </VStack>
  )
}
