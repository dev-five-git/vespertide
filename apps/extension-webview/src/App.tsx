import React, { useState, useEffect } from 'react';
import { Box, Flex, VStack } from '@devup-ui/react';
import { onMessage } from './vscode';
import type { HostMessage, OrmType, Schema } from './vscode';
import OrmEditor from './tabs/OrmEditor';
import MigrationDiff from './tabs/MigrationDiff';
import Export from './tabs/Export';
import defaultSchemasJson from './defaultSchemas.json';

type Tab = 'editor' | 'migration' | 'export';

export const DEFAULT_SCHEMAS: Record<OrmType, string> = defaultSchemasJson;

const TABS: { id: Tab; label: string }[] = [
  { id: 'editor',    label: 'ORM Editor' },
  { id: 'migration', label: 'Migration' },
  { id: 'export',    label: 'Export' },
];

export interface AppState {
  ormSource: string;
  ormType: OrmType;
  svg: string;
  schema: Schema;
  postgres: string;
  mysql: string;
  sqlite: string;
  error: string | null;
  theme: 'dark' | 'light';
}

const INITIAL: AppState = {
  ormSource: DEFAULT_SCHEMAS.prisma,
  ormType: 'prisma',
  svg: '',
  schema: {},
  postgres: '',
  mysql: '',
  sqlite: '',
  error: null,
  theme: 'dark',
};

export default function App() {
  const [tab, setTab] = useState<Tab>('editor');
  const [state, setState] = useState<AppState>(INITIAL);

  useEffect(() => {
    return onMessage((msg: HostMessage) => {
      setState((prev) => {
        switch (msg.type) {
          case 'erd_updated':
            return { ...prev, svg: msg.svg, error: null };
          case 'migration_updated':
            return { ...prev, postgres: msg.postgres, mysql: msg.mysql, sqlite: msg.sqlite, error: null };
          case 'export_done':
            return { ...prev, error: null };
          case 'error':
            return { ...prev, error: msg.message };
          default:
            return prev;
        }
      });
    });
  }, []);

  return (
    <VStack data-theme={state.theme} h="100vh">
      <Flex
        role="tablist"
        alignItems="stretch"
        borderBottom="1px solid $border"
        bg="$tabsBg"
        flexShrink={0}
      >
        {TABS.map(({ id, label }) => (
          <Box
            key={id}
            as="button"
            role="tab"
            aria-selected={tab === id}
            onClick={() => setTab(id)}
            flex={1}
            py="8px"
            px="4px"
            border="none"
            borderBottom={tab === id ? '2px solid $focusBorder' : '2px solid transparent'}
            bg="transparent"
            color={tab === id ? '$fg' : '$inactiveFg'}
            fontSize="11px"
            fontWeight={tab === id ? 600 : 400}
            cursor="pointer"
            transition="color 0.1s, border-color 0.1s"
          >
            {label}
          </Box>
        ))}

        {/* Theme toggle */}
        <Box
          as="button"
          onClick={() => setState((p) => ({ ...p, theme: p.theme === 'dark' ? 'light' : 'dark' }))}
          title={state.theme === 'dark' ? '라이트 모드로 전환' : '다크 모드로 전환'}
          w="36px"
          flexShrink={0}
          border="none"
          borderBottom="2px solid transparent"
          bg="transparent"
          color="$inactiveFg"
          cursor="pointer"
          display="flex"
          alignItems="center"
          justifyContent="center"
        >
          {state.theme === 'dark'
            ? <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41"/></svg>
            : <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round"><path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/></svg>
          }
        </Box>
      </Flex>

      {state.error && (
        <Box
          py="6px"
          px="12px"
          bg="$errBg"
          color="$errFg"
          fontSize="12px"
          flexShrink={0}
        >
          {state.error}
        </Box>
      )}

      <Box flex={1} overflow="hidden">
        {tab === 'editor'    && <OrmEditor    state={state} setState={setState} />}
        {tab === 'migration' && <MigrationDiff state={state} setState={setState} />}
        {tab === 'export'    && <Export       state={state} setState={setState} />}
      </Box>
    </VStack>
  );
}
