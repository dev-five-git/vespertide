import { Box, css } from '@devup-ui/react'

import { Effect } from '@/components/header/effect'
import { Search as SearchComponent } from '@/components/search'
import { SearchForm } from '@/components/search/form'
import {
  SheetRouteContainer,
  SheetRouteTrigger,
} from '@/components/sheet/router'

export function Search() {
  return (
    <>
      <SheetRouteContainer
        className={css({
          display: 'flex',
          alignItems: 'center',
          borderRadius: '0px',
          h: '68px',
          py: '$spacingSpacing12',
          pl: '$spacingSpacing04',
          pr: '$spacingSpacing20',
          gap: '4px',
        })}
        name="search"
        position="top"
      >
        <SheetRouteTrigger name="search">
          <Effect>
            <Box
              aspectRatio="1"
              bg="$title"
              boxSize="24px"
              maskImage="url('/icons/close.svg')"
              maskPos="center"
              maskRepeat="no-repeat"
              maskSize="contain"
            />
          </Effect>
        </SheetRouteTrigger>
        <SearchForm>
          <SearchComponent id="mobile-search" />
        </SearchForm>
      </SheetRouteContainer>
    </>
  )
}
