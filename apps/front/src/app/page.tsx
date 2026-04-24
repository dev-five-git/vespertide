import { JoinIconButton } from '@app/_components/join-icon-button'
import { Box, Center, css, Flex, Text, VStack } from '@devup-ui/react'
import { Image } from '@devup-ui/react'
import type { Metadata } from 'next'
import Link from 'next/link'

import { Button } from '@/components/button'
import { GnbIcon } from '@/components/header/gnb-icon'
import { HeaderSentinel } from '@/components/header/header-sentinel'

import {
  ExampleContainer,
  ExampleImage,
  ExampleProvider,
} from './_components/example'

export const metadata: Metadata = {
  alternates: {
    canonical: '/',
  },
}

const EXAMPLES = [
  {
    id: '1',
    title: 'How to Use',
    description:
      'Lorem ipsum dolor sit amet. Etiam sit amet feugiat turpis. Proin nec ante a sem vestibulum sodales non ut ex.',
    imageUrl: '/images/hero-figure.webp',
  },
  {
    id: '2',
    title: 'How to Use',
    description:
      'Lorem ipsum dolor sit amet. Etiam sit amet feugiat turpis. Proin nec ante a sem vestibulum sodales non ut ex.',
    imageUrl: '/images/join-us-bg.webp',
  },
  {
    id: '3',
    title: 'How to Use',
    description:
      'Lorem ipsum dolor sit amet. Etiam sit amet feugiat turpis. Proin nec ante a sem vestibulum sodales non ut ex.',
    imageUrl: '/images/code.webp',
  },
]

export default function HomePage() {
  return (
    <>
      <Box bg="$containerBackground" color="#FFF" minH="100vh">
        <Center
          bg="linear-gradient(-180deg, #9AE99D 0%, #FFF6DF 100%)"
          flexDir="column"
          h="1080px"
          overflow="hidden"
          pb="60px"
          pos="relative"
          pt="128px"
          px="40px"
        >
          <VStack alignItems="flex-start" gap="50px" maxW="1280px" w="100%">
            <VStack alignItems="flex-start" gap="$spacingSpacing32" w="100%">
              <Text color="#10131F" textAlign="center" typography="displaySm">
                Lorem ipsum dolor sit amet, <br />
                consectetur adipiscing elit.
              </Text>
              <Text color="#10131F" typography="title">
                Etiam sit amet feugiat turpis. Proin nec ante a sem vestibulum
                sodales non ut ex. <br />
                Morbi diam turpis, fringilla vitae enim et, egestas consequat
                nibh. <br />
                Etiam auctor cursus urna sit amet elementum.
              </Text>
            </VStack>
            <Button>Get started</Button>
          </VStack>
          <Box
            bg="url(/images/hero-figure.webp) center/cover no-repeat"
            boxSize="750px"
            left="1028px"
            pos="absolute"
            top="173px"
          />
        </Center>

        <Center
          bg="$vespertideBg"
          flexDir="column"
          overflow="hidden"
          px="20px"
          py="$spacingSpacing80"
        >
          <VStack gap="40px" maxW="1280px" w="100%">
            <VStack gap="16px">
              <Text color="$title" typography="h3">
                Title
              </Text>
              <Text color="$text" typography="body">
                Lorem ipsum dolor sit amet, consectetur adipiscing elit. Nullam
                venenatis, elit in hendrerit porta, augue ante scelerisque diam,{' '}
                <br />
                ac egestas lacus est nec urna. Cras commodo risus hendrerit,
                suscipit nibh at, porttitor dui.
              </Text>
            </VStack>
            <VStack flexDir={[null, null, null, 'row']} gap={5}>
              {[0, 1, 2, 3].map((i) => (
                <Flex
                  key={i}
                  bg="$containerBackground"
                  borderRadius="$spacingSpacing08"
                  minH={['200px', null, null, '320px']}
                  overflow="hidden"
                  px={['$spacingSpacing20', null, null, '$spacingSpacing24']}
                  py={['$spacingSpacing16', null, null, '$spacingSpacing24']}
                >
                  <VStack
                    flex="1"
                    gap={['10px', null, null, '$spacingSpacing12']}
                  >
                    <Text color="$title" typography="title">
                      Feature title
                    </Text>
                    <Text color="$textSub" typography="body">
                      Lorem ipsum dolor sit amet. Etiam sit amet feugiat turpis.
                      Proin nec ante a sem vestibulum sodales non ut ex.{' '}
                    </Text>
                  </VStack>
                </Flex>
              ))}
            </VStack>
          </VStack>
        </Center>

        <ExampleProvider defaultSelected={EXAMPLES[0].id} examples={EXAMPLES}>
          <HeaderSentinel
            className={css({
              display: 'flex',
              justifyContent: 'center',
              alignItems: 'center',
              bg: '$containerBackground',
              flexDir: 'column',
              overflow: 'hidden',
              px: '20px',
              py: ['80px', null, null, '120px'],
            })}
          >
            <VStack gap="40px" maxW={[null, null, null, '1280px']} w="100%">
              <VStack gap="16px">
                <Text color="$title" typography="h3">
                  Title
                </Text>
                <Text color="$title" typography="body">
                  Lorem ipsum dolor sit amet, consectetur adipiscing elit.
                  Nullam venenatis ac egestas lacus est nec urna.{' '}
                </Text>
              </VStack>
              <VStack
                alignItems="center"
                flexDir={[null, null, null, 'row-reverse']}
                gap="$spacingSpacing32"
                pos="relative"
              >
                <Flex
                  bg="$vespertideBg"
                  borderRadius="$spacingSpacing08"
                  flexShrink="0"
                  h={['320px', null, null, '424px']}
                  justifyContent="center"
                  overflow="hidden"
                  pos="relative"
                  px="$spacingSpacing20"
                  py="20px"
                  w={['100%', null, null, '624px']}
                >
                  <ExampleImage />
                  <Box
                    bottom="27px"
                    left="50%"
                    pos="absolute"
                    transform="translateX(-50%)"
                  >
                    <Button>Learn more</Button>
                  </Box>
                </Flex>
                <VStack gap="$spacingSpacing12" w="100%">
                  {EXAMPLES.map(({ id, title, description }) => (
                    <ExampleContainer key={id} value={id}>
                      <VStack
                        flex="1"
                        gap={['10px', null, null, '$spacingSpacing12']}
                      >
                        <Text color="$title" typography="title">
                          {title}
                        </Text>
                        <Text color="$textSub" typography="body">
                          {description}
                        </Text>
                      </VStack>
                    </ExampleContainer>
                  ))}
                </VStack>
              </VStack>
            </VStack>
          </HeaderSentinel>
        </ExampleProvider>

        <HeaderSentinel
          className={css({
            alignItems: 'center',
            display: 'flex',
            flexDir: 'column',
            bg: '#000',
            gap: '40px',
            overflow: 'hidden',
            pos: 'relative',
            px: ['20px', null, null, '40px'],
            py: ['80px', null, null, '120px'],
            h: ['600px', null, null, 'unset'],
          })}
        >
          <VStack
            flexDir={[null, null, null, 'row']}
            h={['100%', null, null, 'unset']}
            justifyContent={[null, null, null, 'flex-end']}
            maxW="1280px"
            pos="relative"
            w="100%"
          >
            <VStack
              gap="40px"
              justifyContent="center"
              maxW="480px"
              w="100%"
              zIndex="10"
            >
              <VStack gap="16px">
                <Text color="#FFF" typography="h3">
                  Join our community
                </Text>
                <Text color="#FFF" typography="body">
                  Join our Discord and help build the future of frontend with
                  CSS-in-JS!{' '}
                </Text>
              </VStack>
              <Flex alignItems="center" gap="16px">
                <Link
                  href="https://discord.com/invite/8zjcGc7cWh"
                  rel="noopener noreferrer"
                  target="_blank"
                >
                  <JoinIconButton>
                    <GnbIcon
                      className={css({ bg: '$vespertidePrimary' })}
                      icon="discord"
                    />
                  </JoinIconButton>
                </Link>
                <Link
                  href="https://open.kakao.com/o/giONwVAh"
                  rel="noopener noreferrer"
                  target="_blank"
                >
                  <JoinIconButton>
                    <GnbIcon
                      className={css({ bg: '$vespertidePrimary' })}
                      icon="kakao"
                    />
                  </JoinIconButton>
                </Link>
                <Link
                  href="https://devfive.kr"
                  rel="noopener noreferrer"
                  target="_blank"
                >
                  <JoinIconButton>
                    <GnbIcon
                      className={css({ bg: '$vespertidePrimary' })}
                      icon="devfive"
                    />
                  </JoinIconButton>
                </Link>
              </Flex>
            </VStack>
            <Image
              bottom="-305px"
              boxSize="500px"
              left="-142px"
              pos="absolute"
              src="/images/join-us-bg.webp"
            />
          </VStack>
        </HeaderSentinel>
      </Box>
    </>
  )
}
