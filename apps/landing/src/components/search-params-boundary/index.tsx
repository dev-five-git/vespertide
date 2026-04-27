'use client'

import { useSearchParams } from 'next/navigation'
import { Suspense } from 'react'

interface SearchParamsBoundaryProps {
  children?: React.ReactNode
  reverse?: boolean
  queryKey: string
  candidates?: string[]
}

function Inner({
  children,
  reverse = false,
  queryKey,
  candidates,
}: SearchParamsBoundaryProps) {
  const searchParams = useSearchParams()
  const value = searchParams.get(queryKey)
  const pass = value ? (candidates ? candidates.includes(value) : true) : false
  if (reverse) return pass ? null : children
  return pass ? children : null
}

export function SearchParamsBoundary(props: SearchParamsBoundaryProps) {
  return (
    <Suspense>
      <Inner {...props} />
    </Suspense>
  )
}
