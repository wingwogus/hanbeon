import { PageSection } from '@components/Layout/PageSection'
import { LogReport } from '@components/Report/LogReport'

export default function Page() {
  return (
    <PageSection
      caption="데스크톱 앱이 남긴 기록을 읽어 실증 지표를 봅니다. 파일은 브라우저 안에서만 열리고 서버로 올라가지 않습니다."
      maxW
      title="실증 기록"
    >
      <LogReport />
    </PageSection>
  )
}
