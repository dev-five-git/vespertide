'use client'

import { Center, css, Grid, Text, VStack } from '@devup-ui/react'
import Link from 'next/link'
import { useSearchParams } from 'next/navigation'
import { ComponentProps, useEffect, useMemo, useState } from 'react'

export function Result(
  props: Omit<ComponentProps<typeof VStack<'div'>>, 'children'>,
) {
  const query = useSearchParams().get('search')
  const [data, setData] = useState<
    {
      title: string
      text: string
      url: string
    }[]
  >()
  useEffect(() => {
    if (query) {
      fetch('/search.json')
        .then((response) => response.json())
        .then(
          (
            data: {
              title: string
              text: string
              url: string
            }[],
          ) => {
            setData(
              data
                .filter(Boolean)
                .filter(
                  (item) =>
                    item.title.toLowerCase().includes(query.toLowerCase()) ||
                    item.text.toLowerCase().includes(query.toLowerCase()),
                ),
            )
          },
        )
    }
  }, [query])
  const reg = useMemo(() => new RegExp(`(${query})`, 'gi'), [query])
  if (!query) return
  return (
    <Grid
      bg="$containerBackground"
      borderRadius="16px"
      display="none"
      h="max-content"
      left="50%"
      pl="$spacingSpacing08"
      pos="absolute"
      pr="$spacingSpacing16"
      py="$spacingSpacing20"
      selectors={{
        'body:has(#desktop-search:focus), body:has(#mobile-search) &': {
          display: 'block',
        },
      }}
      styleOrder={1}
      top="88px"
      transform="translateX(-50%)"
      w={['100%', null, '500px']}
      zIndex="120"
      {...props}
    >
      <VStack
        bg="$containerBackground"
        h="max-content"
        maxH="800px"
        overflowY="auto"
        selectors={{
          '&::-webkit-scrollbar-thumb': {
            bg: '$border',
            borderRadius: '8px',
            border: '16px solid transparent',
          },
          '&::-webkit-scrollbar': {
            w: '6px',
          },
        }}
        w={[null, null, '468px']}
      >
        {data?.length ? (
          <VStack gap="16px">
            {data.map((item, i) => (
              <Link
                key={item.url + i}
                className={css({ display: 'contents' })}
                href={item.url}
              >
                <VStack>
                  <Text color="$title" px="16px" typography="tinyB">
                    {item.url}
                  </Text>
                  <VStack px="16px" py="12px">
                    <Text color="$title" typography="menu">
                      {item.title}
                    </Text>
                    <Text
                      WebkitBoxOrient="vertical"
                      WebkitLineClamp="6"
                      color="$title"
                      display="-webkit-box"
                      overflow="hidden"
                      textOverflow="ellipsis"
                      typography="caption"
                    >
                      {item.text.split(reg).map((part, idx) =>
                        part.toLowerCase() === query.toLowerCase() ? (
                          <Text
                            key={idx}
                            color="$vespertidePrimary"
                            fontWeight="bold"
                          >
                            {part}
                          </Text>
                        ) : (
                          <Text key={idx} as="span">
                            {part}
                          </Text>
                        ),
                      )}
                    </Text>
                  </VStack>
                </VStack>
              </Link>
            ))}
          </VStack>
        ) : (
          <Center py="40px">
            <Text color="$caption" textAlign="center" typography="caption">
              No search results found.
            </Text>
          </Center>
        )}
      </VStack>
    </Grid>
  )
}

export { Result as SearchResult }
