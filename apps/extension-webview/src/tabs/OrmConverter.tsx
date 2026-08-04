import React, { useState } from 'react';
import { Box, Flex, VStack } from '@devup-ui/react';
import { postMessage } from '../vscode';
import type { OrmType } from '../vscode';
import type { AppState } from '../App';

interface Props {
  state: AppState;
  setState: React.Dispatch<React.SetStateAction<AppState>>;
}

const ORM_LABELS: Record<OrmType, string> = {
  prisma:     'Prisma',
  typeorm:    'TypeORM',
  drizzle:    'Drizzle',
  jpa:        'JPA',
  sqlalchemy: 'SQLAlchemy',
  gorm:       'GORM',
};

const ORM_TYPES = Object.keys(ORM_LABELS) as OrmType[];

export default function OrmConverter({ state, setState }: Props) {
  const [target, setTarget] = useState<OrmType | null>(null);

  const canConvert = target !== null && target !== state.ormType && !!state.ormSource;

  const handleConvert = () => {
    if (!canConvert || !target) return;
    postMessage({ type: 'convert_orm', source: state.ormSource, from: state.ormType, to: target });
    setState((prev) => ({ ...prev, ormType: target }));
    setTarget(null);
  };

  return (
    <VStack
      p="16px"
      h="100%"
      overflow="auto"
      gap="16px"
    >
      {/* ORM buttons */}
      <Box>
        <Box fontSize="11px" opacity={0.55} mb="8px" letterSpacing="0.03em">
          변환할 대상 ORM 선택
        </Box>
        <Flex flexWrap="wrap" gap="8px">
          {ORM_TYPES.map((orm) => {
            const isCurrent = orm === state.ormType;
            const isTarget  = orm === target;
            return (
              <Box
                key={orm}
                as="button"
                onClick={() => !isCurrent && setTarget(isTarget ? null : orm)}
                disabled={isCurrent}
                title={isCurrent ? '현재 ORM' : `${ORM_LABELS[orm]}으로 변환`}
                py="6px"
                px="14px"
                border="1px solid"
                borderColor={
                  isCurrent ? '$focusBorder'
                  : isTarget ? '$chartsGreen'
                  : '$inputBorder'
                }
                borderRadius="4px"
                bg={
                  isCurrent ? '$btnBg'
                  : isTarget ? 'rgba(76,175,80,0.15)'
                  : 'transparent'
                }
                color={
                  isCurrent ? '$btnFg'
                  : '$fg'
                }
                fontSize="12px"
                opacity={isCurrent ? 1 : 0.9}
                cursor={isCurrent ? 'not-allowed' : 'pointer'}
                transition="all 0.1s"
              >
                {ORM_LABELS[orm]}
                {isCurrent && (
                  <Box as="span" ml="6px" fontSize="10px" opacity={0.7}>현재</Box>
                )}
              </Box>
            );
          })}
        </Flex>
      </Box>

      {/* Conversion confirmation bar */}
      {target && target !== state.ormType && (
        <Flex
          py="12px"
          px="16px"
          borderRadius="4px"
          bg="$widgetBg"
          border="1px solid $border"
          alignItems="center"
          justifyContent="space-between"
          gap="12px"
        >
          <Box as="span" fontSize="12px">
            <strong>{ORM_LABELS[state.ormType]}</strong>
            {' → '}
            <strong>{ORM_LABELS[target]}</strong>
            {'  으로 변환'}
          </Box>
          <Box
            as="button"
            onClick={handleConvert}
            disabled={!canConvert}
            py="6px"
            px="18px"
            bg={canConvert
              ? '$btnBg'
              : '$btnSecBg'}
            color="$btnFg"
            border="none"
            borderRadius="3px"
            fontSize="12px"
            cursor={canConvert ? 'pointer' : 'not-allowed'}
            opacity={canConvert ? 1 : 0.5}
            whiteSpace="nowrap"
          >
            변환
          </Box>
        </Flex>
      )}

      {!state.ormSource && (
        <Box as="p" m={0} opacity={0.5} fontSize="12px">
          ORM Editor 탭에서 먼저 스키마 코드를 입력하세요.
        </Box>
      )}

      {/* Current source preview */}
      {state.ormSource && (
        <VStack flex={1} overflow="hidden">
          <Box fontSize="11px" opacity={0.55} mb="6px">
            현재 스키마 ({ORM_LABELS[state.ormType]})
          </Box>
          <Box
            as="pre"
            m={0}
            p="12px"
            flex={1}
            overflow="auto"
            bg="$editorBg"
            borderRadius="4px"
            fontSize="12px"
            fontFamily="$editorFont"
            color="$editorFg"
            lineHeight={1.5}
            whiteSpace="pre"
          >
            {state.ormSource}
          </Box>
        </VStack>
      )}
    </VStack>
  );
}
