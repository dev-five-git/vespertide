'use client'
import { Center, Text } from '@devup-ui/react'
import { usePathname } from 'next/navigation'

export function Menu({
  children,
  value,
}: {
  children?: React.ReactNode
  value: string
}) {
  const pathname = usePathname()
  const isSelected = pathname.includes(value)
  return (
    <Center cursor="pointer" px="$spacingSpacing24" py="$spacingSpacing08">
      <Text
        _hover={{
          color: '$textSub',
        }}
        color={isSelected ? '$vespertidePrimary' : '$menutext'}
        transition="all .1s"
        typography="menu"
        whiteSpace="nowrap"
      >
        {children}
      </Text>
    </Center>
  )
}
