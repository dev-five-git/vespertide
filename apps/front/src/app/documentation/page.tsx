import type { Metadata } from 'next'
import { redirect } from 'next/navigation'

export const metadata: Metadata = {
  title: 'Vespertide - Documentation',
  description: 'Vespertide documentation',
  alternates: {
    canonical: '/documentation',
  },
  openGraph: {
    title: 'Vespertide - Documentation',
    description: 'Vespertide documentation',
    url: '/documentation',
    siteName: 'Vespertide',
  },
}

export default function Page() {
  redirect('/documentation/overview')
}
