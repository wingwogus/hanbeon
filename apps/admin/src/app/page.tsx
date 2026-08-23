import Link from 'next/link'

export default function Page() {
  return (
    <>
      AuthRedirect 사용
      <br />
      <Link href="/signin">로그인 페이지로</Link>
      <br />
      <Link href="/dashboard">대시보드로</Link>
    </>
  )
}
