'use client'

import { Box, css, Text, VStack } from '@devup-ui/react'
import { useState } from 'react'

import type { IntervalPoint } from '@/lib/metrics'

const W = 640
const H = 200
const PAD = { top: 20, right: 44, bottom: 28, left: 44 }

const seconds = (ms: number) => `${(ms / 1000).toFixed(1)}초`

/**
 * 주사 간격이 세션 동안 어떻게 움직였는지.
 *
 * 계단으로 그린다. 간격은 조정된 순간에 값이 바뀌고 그 다음 조정까지 그대로
 * 유지되므로, 점 사이를 비스듬히 이으면 있지도 않은 중간값이 있는 것처럼 보인다.
 */
export function IntervalChart({
  points,
  title,
  caption,
}: {
  points: IntervalPoint[]
  title: string
  caption?: string
}) {
  const [hovered, setHovered] = useState<number | null>(null)

  if (points.length === 0) return null

  const plotW = W - PAD.left - PAD.right
  const plotH = H - PAD.top - PAD.bottom

  const lastAt = Math.max(1, points[points.length - 1].atMs)
  const values = points.map((point) => point.intervalMs)
  const low = Math.min(...values)
  const high = Math.max(...values)
  // 값이 하나뿐이면 위아래로 여유를 줘야 선이 테두리에 붙지 않는다.
  const span = Math.max(200, high - low)

  const x = (atMs: number) => PAD.left + (atMs / lastAt) * plotW
  const y = (ms: number) =>
    PAD.top + plotH - ((ms - low + span * 0.15) / (span * 1.3)) * plotH

  // 계단: 다음 점의 x까지 값을 유지한 뒤 세로로 옮긴다.
  const path = points
    .map((point, index) =>
      index === 0
        ? `M ${x(point.atMs)} ${y(point.intervalMs)}`
        : `L ${x(point.atMs)} ${y(points[index - 1].intervalMs)} L ${x(point.atMs)} ${y(point.intervalMs)}`,
    )
    .join(' ')

  const changes = points.filter((point) => point.reason)

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
          <line
            className={css({ stroke: '$border' })}
            strokeWidth={1}
            x1={PAD.left}
            x2={W - PAD.right}
            y1={PAD.top + plotH}
            y2={PAD.top + plotH}
          />

          <path
            className={css({ stroke: '$chartAccent' })}
            d={path}
            fill="none"
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
          />

          {points.map((point, index) => {
            const cx = x(point.atMs)
            const cy = y(point.intervalMs)
            const adjusted = Boolean(point.reason)

            return (
              <g
                key={`${point.atMs}-${index}`}
                onMouseEnter={() => setHovered(index)}
                onMouseLeave={() => setHovered(null)}
              >
                <circle cx={cx} cy={cy} fill="transparent" r={14} />
                {adjusted && (
                  <circle
                    className={css({
                      fill: '$chartAccent',
                      stroke: '$containerBackground',
                    })}
                    cx={cx}
                    cy={cy}
                    r={5}
                    strokeWidth={2}
                  />
                )}
              </g>
            )
          })}

          {/* 처음과 마지막 값만 직접 적는다. 모든 점에 숫자를 얹으면
              정작 형태가 보이지 않는다. */}
          <text
            className={css({ fill: '$caption' })}
            fontSize={11}
            textAnchor="end"
            x={PAD.left - 6}
            y={y(points[0].intervalMs) + 4}
          >
            {seconds(points[0].intervalMs)}
          </text>
          <text
            className={css({ fill: '$text' })}
            fontSize={11}
            textAnchor="start"
            x={W - PAD.right + 6}
            y={y(points[points.length - 1].intervalMs) + 4}
          >
            {seconds(points[points.length - 1].intervalMs)}
          </text>

          <text
            className={css({ fill: '$caption' })}
            fontSize={11}
            textAnchor="start"
            x={PAD.left}
            y={H - 10}
          >
            세션 시작
          </text>
          <text
            className={css({ fill: '$caption' })}
            fontSize={11}
            textAnchor="end"
            x={W - PAD.right}
            y={H - 10}
          >
            {seconds(lastAt)} 뒤
          </text>
        </svg>

        {hovered !== null && points[hovered] && (
          <Box
            bg="$containerBackground"
            border="1px solid $border"
            borderRadius="8px"
            boxShadow="0 2px 8px rgba(0,0,0,0.12)"
            left={`${(points[hovered].atMs / lastAt) * 100}%`}
            maxW="260px"
            pointerEvents="none"
            pos="absolute"
            px="10px"
            py="6px"
            top="0"
            transform="translateX(-50%)"
          >
            <Text color="$text" typography="bodyS">
              {points[hovered].reason ?? seconds(points[hovered].intervalMs)}
            </Text>
          </Box>
        )}
      </Box>

      {/* 그림만으로는 '왜 바뀌었는지'를 알 수 없다. 이유는 글로 남긴다. */}
      {changes.length > 0 && (
        <VStack gap="2px" pl="2px">
          {changes.map((point) => (
            <Text
              key={`${point.atMs}-${point.reason}`}
              color="$caption"
              typography="bodyS"
            >
              {seconds(point.atMs)} · {point.reason}
            </Text>
          ))}
        </VStack>
      )}
    </VStack>
  )
}
