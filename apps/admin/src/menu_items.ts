type MenuItem = {
  label: string
  items: {
    id: string
    label: string
    link?: string
    childrens?: {
      id: string
      link?: string
      label: string
    }[]
  }[]
}

export const MENU_ITEMS: MenuItem[] = [
  {
    label: '실증',
    items: [
      {
        id: 'log-report',
        label: '기록 요약',
        link: '/dashboard',
      },
    ],
  },
]
