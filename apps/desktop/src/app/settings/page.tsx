'use client'

import { Center, Text } from '@devup-ui/react'
import { useEffect, useState } from 'react'

import { Onboarding } from '@/components/settings/Onboarding'
import { SettingsForm } from '@/components/settings/SettingsForm'
import { getProfile, type Profile } from '@/lib/profile'

export default function SettingsPage() {
  const [profile, setProfile] = useState<Profile | null>(null)
  const [failed, setFailed] = useState(false)

  useEffect(() => {
    getProfile()
      .then(setProfile)
      .catch(() => setFailed(true))
  }, [])

  if (failed) {
    return (
      <Center bg="$background" minH="100vh" p="32px">
        <Text color="$text" typography="bodyXL">
          설정을 읽지 못했습니다. 앱을 다시 시작해 주세요.
        </Text>
      </Center>
    )
  }

  if (!profile) {
    return (
      <Center bg="$background" minH="100vh" p="32px">
        <Text color="$caption" typography="bodyXL">
          설정을 불러오는 중입니다
        </Text>
      </Center>
    )
  }

  // 첫 실행이면 3단계 안내부터. 마치고 나면 같은 창이 설정 화면으로 바뀐다.
  return profile.onboarded ? (
    <SettingsForm initial={profile} />
  ) : (
    <Onboarding initial={profile} onDone={setProfile} />
  )
}
