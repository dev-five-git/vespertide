import { Box, Flex, Text, VStack } from '@devup-ui/react'
import type { Metadata } from 'next'
import Link from 'next/link'
import type { ComponentProps } from 'react'

import { Button } from '@/components/button'

import { CodeTabs, type CodeExampleQuad } from './_components/code-tabs'
import { CodeWindow, HighlightedCode } from './_components/code-window'
import { CopyInstall } from './_components/copy-install'
import { highlight } from './_lib/highlight'

export const metadata: Metadata = {
  alternates: {
    canonical: '/',
  },
}

const VERSION = '0.1.61'
const GITHUB_URL = 'https://github.com/dev-five-git/vespertide'
const DOCS_URL = '/documentation'
const DISCORD_URL = 'https://discord.com/invite/8zjcGc7cWh'
const KAKAO_URL = 'https://open.kakao.com/o/giONwVAh'
const CRATES_URL = 'https://crates.io/crates/vespertide-cli'

const HERO_MODEL_JSON = `{
  "$schema": "https://raw.githubusercontent.com/dev-five-git/vespertide/main/schemas/model.schema.json",
  "name": "user",
  "columns": [
    { "name": "id", "type": "integer", "primary_key": true },
    { "name": "email", "type": "text", "unique": true, "index": true },
    { "name": "name", "type": { "kind": "varchar", "length": 100 } },
    {
      "name": "status",
      "type": { "kind": "enum", "name": "user_status",
                "values": ["active", "inactive", "banned"] },
      "default": "'active'"
    },
    { "name": "created_at", "type": "timestamptz", "default": "NOW()" }
  ]
}`

const EXAMPLE_MODEL = `{
  "$schema": "https://raw.githubusercontent.com/dev-five-git/vespertide/main/schemas/model.schema.json",
  "name": "post",
  "columns": [
    { "name": "id", "type": "integer", "primary_key": true },
    { "name": "title", "type": { "kind": "varchar", "length": 200 } },
    { "name": "body", "type": "text" },
    {
      "name": "author_id",
      "type": "integer",
      "foreign_key": {
        "ref_table": "user",
        "ref_columns": ["id"],
        "on_delete": "cascade"
      },
      "index": true
    },
    {
      "name": "status",
      "type": { "kind": "enum", "name": "post_status",
                "values": ["draft", "published", "archived"] },
      "default": "'draft'"
    }
  ]
}`

const EXAMPLE_CLI = `# Initialize a new project
$ vespertide init

# Scaffold a model
$ vespertide new post

# Edit models/post.json, then preview the diff
$ vespertide diff
+ create_table post (id, title, body, author_id, status)
+ create_enum post_status [draft, published, archived]
+ create_index ix_post_author_id ON post (author_id)

# Inspect dialect-specific SQL
$ vespertide sql --backend postgres

# Persist as a migration file
$ vespertide revision -m "create post table"`

const EXAMPLE_RUNTIME = `use sea_orm::Database;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = Database::connect("postgres://user:pass@localhost/mydb").await?;

    // Generated at compile time, run on startup.
    vespertide::vespertide_migration!(db).await?;

    Ok(())
}`

const EXAMPLE_EXPORT = `# Generate Rust SeaORM entities
$ vespertide export --orm seaorm

# Or Python — SQLAlchemy
$ vespertide export --orm sqlalchemy

# Or FastAPI-flavoured SQLModel
$ vespertide export --orm sqlmodel

# Or Go — GORM
$ vespertide export --orm gorm`

type FeatureIconName =
  | 'hamburger'
  | 'arrow-up-right'
  | 'chevron'
  | 'logo-image'
  | 'devfive'
  | 'theme-dark'
  | 'search'
  | 'external-link'
  | 'github'

const FEATURES: { icon: FeatureIconName; title: string; desc: string }[] = [
  {
    icon: 'hamburger',
    title: 'Declarative schema',
    desc: 'Describe your desired database state in JSON files. The current model is the source of truth.',
  },
  {
    icon: 'arrow-up-right',
    title: 'Automatic diffing',
    desc: 'Vespertide replays applied migrations and compares them to your models to compute changes.',
  },
  {
    icon: 'chevron',
    title: 'Typed migration plans',
    desc: 'Generates safe, portable MigrationAction enums — not raw SQL. Review before you commit.',
  },
  {
    icon: 'logo-image',
    title: 'Multi-database',
    desc: 'PostgreSQL, MySQL, and SQLite — same schema, identical semantics, backend-aware quoting.',
  },
  {
    icon: 'devfive',
    title: 'Native enums',
    desc: 'First-class string and integer enums. Add new integer values without ever touching the DB.',
  },
  {
    icon: 'theme-dark',
    title: 'Zero-runtime macro',
    desc: 'vespertide_migration!() generates database-specific SQL at compile time. Nothing to ship at runtime.',
  },
  {
    icon: 'search',
    title: 'JSON Schema validation',
    desc: 'Ships with JSON Schemas — autocomplete, hover docs, and instant errors in your editor.',
  },
  {
    icon: 'external-link',
    title: 'ORM export',
    desc: 'One command emits SeaORM, SQLAlchemy, SQLModel, or GORM — entities stay in lockstep with schema.',
  },
  {
    icon: 'github',
    title: 'Built in Rust',
    desc: "Single binary CLI, no Node, no Python, no JVM. cargo install vespertide-cli and you're done.",
  },
]

const STEPS: { title: string; desc: string; mono: string }[] = [
  {
    title: 'Define',
    desc: 'Author JSON models in your editor with full IDE validation via JSON Schema.',
    mono: 'models/user.json',
  },
  {
    title: 'Replay',
    desc: 'Vespertide reconstructs the baseline schema by replaying applied migrations.',
    mono: 'migrations/*.sql',
  },
  {
    title: 'Diff',
    desc: 'Current models are compared to the baseline to find what changed.',
    mono: 'vespertide diff',
  },
  {
    title: 'Plan',
    desc: 'Differences are converted into typed MigrationAction enums.',
    mono: 'MigrationAction',
  },
  {
    title: 'Emit',
    desc: 'Actions translate to dialect-specific SQL — Postgres, MySQL, or SQLite.',
    mono: 'vespertide sql',
  },
]

const DBS = [
  {
    key: 'PG',
    name: 'PostgreSQL',
    quote: '"identifier"',
    note: 'Full feature support — native enums, JSONB, INET, CIDR, TSVECTOR.',
  },
  {
    key: 'MY',
    name: 'MySQL',
    quote: '`identifier`',
    note: 'Full feature support with MySQL-aware identifier quoting and types.',
  },
  {
    key: 'SL',
    name: 'SQLite',
    quote: '"identifier"',
    note: 'Full feature support — perfect for tests, CLIs, and embedded apps.',
  },
]

const ORMS = [
  { lang: 'Rust', name: 'SeaORM' },
  { lang: 'Python', name: 'SQLAlchemy' },
  { lang: 'Python', name: 'SQLModel · FastAPI' },
  { lang: 'Go', name: 'GORM' },
]

function MaskIcon({
  icon,
  size = '24px',
  color = '$vespertidePrimary',
  ...props
}: {
  icon: FeatureIconName | 'discord' | 'kakao'
  size?: string
  color?: string
} & ComponentProps<typeof Box<'div'>>) {
  return (
    <Box
      bg={color}
      boxSize={size}
      maskImage={`url('/icons/${icon}.svg')`}
      maskPos="center"
      maskRepeat="no-repeat"
      maskSize="contain"
      styleOrder={1}
      {...props}
    />
  )
}

function SectionHead({
  eyebrow,
  title,
  emphasis,
  lede,
}: {
  eyebrow: string
  title: string
  emphasis?: string
  lede?: string
}) {
  return (
    <VStack alignItems="flex-start" gap="16px" maxW="720px">
      <Text
        color="$vespertidePrimary"
        textTransform="uppercase"
        typography="eyebrow"
      >
        — {eyebrow}
      </Text>
      <Text
        color="$title"
        display="inline-flex"
        flexWrap="wrap"
        gap="0.3em"
        typography="h2"
      >
        {title}
        {emphasis && (
          <Text color="$vespertidePrimary" fontStyle="italic">
            {emphasis}
          </Text>
        )}
      </Text>
      {lede && (
        <Text color="$textSub" maxW="620px" typography="bodyLg">
          {lede}
        </Text>
      )}
    </VStack>
  )
}

function HeroSection({ codeHtml }: { codeHtml: string }) {
  return (
    <Box
      as="section"
      overflow="hidden"
      pos="relative"
      pt={['120px', null, null, '148px']}
      px={['20px', null, null, '40px']}
    >
      <Box
        bg="radial-gradient(ellipse 50% 50% at 85% 0%, rgba(14,195,92,0.10), transparent 70%)"
        inset="0"
        pointerEvents="none"
        pos="absolute"
      />
      <Box maxW="1240px" mx="auto" pb={['60px', null, null, '80px']} pos="relative">
        <VStack
          alignItems="center"
          flexDir={[null, null, null, 'row']}
          gap={['40px', null, null, '60px']}
        >
          <VStack flex="1.1" w="100%">
            <Flex
              alignItems="center"
              bg="$vespertideBg"
              border="1px solid $border"
              borderRadius="50%"
              gap="8px"
              mb="24px"
              px="12px"
              py="5px"
              w="fit-content"
            >
              <Box
                bg="$vespertidePrimary"
                borderRadius="50%"
                boxShadow="0 0 0 4px rgba(14,195,92,0.18)"
                boxSize="6px"
              />
              <Text color="$textSub" fontFamily="D2Coding" fontSize="12px">
                v{VERSION} · Apache-2.0 · Rust
              </Text>
            </Flex>

            <Text
              color="$title"
              mb="22px"
              typography="displaySm"
            >
              Define schemas.
              <br />
              <Text color="$vespertidePrimary" fontStyle="italic">
                Forget migrations.
              </Text>
            </Text>

            <Box mb="32px">
              <Text color="$textSub" maxW="520px" typography="bodyLg">
                Vespertide is a declarative database schema manager for Rust. Write
                your tables in JSON, and let it diff, plan, and emit type-safe
                migrations to Postgres, MySQL, and SQLite — automatically.
              </Text>
            </Box>

            <Flex flexWrap="wrap" gap="12px" mb="28px">
              <Link href="#quickstart">
                <Button>Get started</Button>
              </Link>
              <Link href={GITHUB_URL} rel="noopener noreferrer" target="_blank">
                <Flex
                  _hover={{ borderColor: '$caption' }}
                  alignItems="center"
                  as="button"
                  bg="transparent"
                  border="1px solid $border"
                  borderRadius="100px"
                  color="$title"
                  cursor="pointer"
                  gap="8px"
                  px={['28px', null, null, null, '$spacingSpacing32']}
                  py={['10px', null, null, null, '$spacingSpacing12']}
                  transition="border-color .15s"
                >
                  <MaskIcon color="$title" icon="github" size="18px" />
                  <Text color="$title" typography="bodyLgEb">
                    View on GitHub
                  </Text>
                </Flex>
              </Link>
            </Flex>

            <CopyInstall />

            <Flex
              borderTop="1px solid $border"
              flexWrap="wrap"
              gap={['20px', null, null, '40px']}
              mt="32px"
              pt="24px"
            >
              {[
                { num: `v${VERSION}`, lbl: 'Version' },
                { num: '3', lbl: 'Databases' },
                { num: '4', lbl: 'ORM exports' },
                { num: '0ms', lbl: 'Runtime cost' },
              ].map((s) => (
                <VStack alignItems="flex-start" gap="4px" key={s.lbl}>
                  <Text color="$title" typography="h3">
                    {s.num}
                  </Text>
                  <Text
                    color="$caption"
                    fontFamily="D2Coding"
                    fontSize="11px"
                    letterSpacing="0.08em"
                    textTransform="uppercase"
                  >
                    {s.lbl}
                  </Text>
                </VStack>
              ))}
            </Flex>
          </VStack>

          <Box flex="1" w="100%">
            <CodeWindow title="models/user.json">
              <HighlightedCode html={codeHtml} />
            </CodeWindow>
          </Box>
        </VStack>
      </Box>
    </Box>
  )
}

function FeaturesSection() {
  return (
    <Box
      as="section"
      bg="$vespertideBg"
      id="features"
      px={['20px', null, null, '40px']}
      py={['64px', null, null, '100px']}
    >
      <Box maxW="1240px" mx="auto">
        <Box mb="56px">
          <SectionHead
            emphasis="review schemas"
            eyebrow="Features"
            lede="Everything Vespertide does is reversible, inspectable, and committed to git as plain JSON. No magic, no runtime surprises."
            title="Built for teams who'd rather"
          />
        </Box>
        <Box
          bg="$border"
          border="1px solid $border"
          borderRadius="$borderRadiusRadius12"
          display="grid"
          gap="1px"
          gridTemplateColumns={['1fr', null, 'repeat(2, 1fr)', 'repeat(3, 1fr)']}
          overflow="hidden"
        >
          {FEATURES.map((f) => (
            <VStack
              _hover={{ bg: '$containerBackground' }}
              alignItems="flex-start"
              bg="$vespertideBg"
              key={f.title}
              px={['20px', null, null, '28px']}
              py={['24px', null, null, '32px']}
              transition="background .15s"
            >
              <MaskIcon
                color="$vespertidePrimary"
                icon={f.icon}
                mb="22px"
                size="28px"
              />
              <Text color="$title" mb="8px" typography="h5">
                {f.title}
              </Text>
              <Text color="$textSub" typography="bodySm">
                {f.desc}
              </Text>
            </VStack>
          ))}
        </Box>
      </Box>
    </Box>
  )
}

function PipelineSection() {
  return (
    <Box
      as="section"
      borderTop="1px solid $border"
      id="how"
      px={['20px', null, null, '40px']}
      py={['64px', null, null, '100px']}
    >
      <Box maxW="1240px" mx="auto">
        <Box mb="56px">
          <SectionHead
            emphasis="JSON to SQL."
            eyebrow="How it works"
            lede="The whole pipeline is deterministic. Every artifact is a plain file you can read, review, and version-control."
            title="Five steps from"
          />
        </Box>
        <Box
          display="grid"
          gap="20px"
          gridTemplateColumns={[
            '1fr',
            null,
            'repeat(2, 1fr)',
            'repeat(5, 1fr)',
          ]}
        >
          {STEPS.map((s, i) => (
            <VStack
              alignItems="flex-start"
              bg="$vespertideBg"
              border="1px solid $border"
              borderRadius="$borderRadiusRadius08"
              key={s.title}
              px="22px"
              py="24px"
            >
              <Text
                color="$vespertidePrimary"
                fontFamily="D2Coding"
                fontSize="11px"
                letterSpacing="0.08em"
              >
                {String(i + 1).padStart(2, '0')}
              </Text>
              <Text color="$title" mb="10px" mt="8px" typography="h4">
                {s.title}
              </Text>
              <Text color="$textSub" mb="12px" typography="bodySm">
                {s.desc}
              </Text>
              <Box borderTop="1px dashed $caption" pt="12px" w="100%">
                <Text
                  color="$caption"
                  fontFamily="D2Coding"
                  fontSize="11px"
                >
                  {s.mono}
                </Text>
              </Box>
            </VStack>
          ))}
        </Box>
      </Box>
    </Box>
  )
}

function ExamplesSection({ examples }: { examples: CodeExampleQuad }) {
  return (
    <Box
      as="section"
      bg="$vespertideBg"
      borderTop="1px solid $border"
      id="examples"
      px={['20px', null, null, '40px']}
      py={['64px', null, null, '100px']}
    >
      <Box maxW="1240px" mx="auto">
        <VStack
          alignItems="flex-start"
          flexDir={[null, null, null, 'row']}
          gap={['40px', null, null, '60px']}
        >
          <VStack alignItems="flex-start" flex="1.1" w="100%">
            <Text
              color="$vespertidePrimary"
              fontFamily="D2Coding"
              fontSize="11px"
              letterSpacing="0.14em"
              mb="16px"
              textTransform="uppercase"
            >
              — Examples
            </Text>
            <Text color="$title" mb="16px" typography="h2">
              One source of truth,
              <br />
              four ways to use it.
            </Text>
            <Text color="$textSub" mb="24px" typography="bodyLg">
              Your JSON models drive the diff, the SQL, the ORM entities, and the
              runtime macro. Pick the workflow that fits your team — Vespertide
              stays consistent.
            </Text>
            <VStack alignItems="flex-start" gap="14px" w="100%">
              {[
                {
                  k: 'Models.',
                  v: 'Inline foreign keys, enums, and constraints — no separate schema language.',
                },
                {
                  k: 'CLI.',
                  v: 'diff, sql, revision, status, log — every step is a plain command.',
                },
                {
                  k: 'Runtime.',
                  v: 'Compile-time macro, zero overhead, no migrations folder shipped to prod.',
                },
                {
                  k: 'Export.',
                  v: 'SeaORM, SQLAlchemy, SQLModel, GORM — typed entities, generated.',
                },
              ].map((it) => (
                <Flex alignItems="flex-start" gap="12px" key={it.k}>
                  <Text
                    color="$vespertidePrimary"
                    flexShrink="0"
                    fontFamily="D2Coding"
                    fontWeight="700"
                  >
                    →
                  </Text>
                  <Text
                    color="$textSub"
                    display="inline-flex"
                    flexWrap="wrap"
                    gap="0.3em"
                    typography="body"
                  >
                    <Text as="span" color="$title" fontWeight="600">
                      {it.k}
                    </Text>
                    {it.v}
                  </Text>
                </Flex>
              ))}
            </VStack>
          </VStack>
          <Box flex="1" w="100%">
            <CodeTabs examples={examples} />
          </Box>
        </VStack>
      </Box>
    </Box>
  )
}

function CompatibilitySection() {
  return (
    <Box
      as="section"
      borderTop="1px solid $border"
      px={['20px', null, null, '40px']}
      py={['64px', null, null, '100px']}
    >
      <Box maxW="1240px" mx="auto">
        <Box mb="40px">
          <SectionHead
            emphasis="one schema."
            eyebrow="Compatibility"
            lede="Vespertide emits backend-specific SQL from the same JSON model. Switch databases for tests, staging, or production without rewriting a thing."
            title="Three databases,"
          />
        </Box>

        <Box
          display="grid"
          gap="16px"
          gridTemplateColumns={['1fr', null, null, 'repeat(3, 1fr)']}
        >
          {DBS.map((db) => (
            <VStack
              alignItems="flex-start"
              bg="$vespertideBg"
              border="1px solid $border"
              borderRadius="$borderRadiusRadius12"
              gap="12px"
              key={db.name}
              p="28px"
            >
              <Flex alignItems="center" gap="12px">
                <Flex
                  alignItems="center"
                  bg="$cardBase"
                  border="1px solid $border"
                  borderRadius="8px"
                  boxSize="40px"
                  justifyContent="center"
                >
                  <Text
                    color="$vespertidePrimary"
                    fontFamily="D2Coding"
                    fontWeight="600"
                  >
                    {db.key}
                  </Text>
                </Flex>
                <Text color="$title" typography="h4">
                  {db.name}
                </Text>
              </Flex>
              <Text
                alignSelf="flex-start"
                bg="$cardBase"
                border="1px solid $border"
                borderRadius="4px"
                color="$caption"
                fontFamily="D2Coding"
                fontSize="11px"
                px="10px"
                py="6px"
              >
                {db.quote}
              </Text>
              <Text color="$textSub" mt="auto" typography="bodySm">
                {db.note}
              </Text>
            </VStack>
          ))}
        </Box>

        <Box mt="60px">
          <Text
            color="$vespertidePrimary"
            mb="8px"
            textTransform="uppercase"
            typography="eyebrow"
          >
            — ORM export
          </Text>
          <Text color="$title" mb="6px" typography="h3">
            Generate typed entities for the runtime you use.
          </Text>
          <Text color="$textSub" maxW="560px" typography="body">
            <Text as="span" color="$vespertidePrimary" fontFamily="D2Coding">
              vespertide export --orm &lt;target&gt;
            </Text>{' '}
            emits up-to-date entities from your current models.
          </Text>
          <Flex flexWrap="wrap" gap="12px" mt="32px">
            {ORMS.map((orm) => (
              <Flex
                _hover={{ borderColor: '$vespertidePrimary', color: '$title' }}
                alignItems="center"
                bg="$vespertideBg"
                border="1px solid $border"
                borderRadius="50%"
                color="$textSub"
                fontFamily="D2Coding"
                fontSize="12px"
                gap="8px"
                key={orm.name}
                px="16px"
                py="10px"
                transition="all .15s"
              >
                <Text color="$vespertidePrimary" fontFamily="D2Coding">
                  {orm.lang}
                </Text>
                <Text fontFamily="D2Coding">{orm.name}</Text>
              </Flex>
            ))}
          </Flex>
        </Box>
      </Box>
    </Box>
  )
}

function ChannelRow({
  href,
  icon,
  name,
  meta,
}: {
  href: string
  icon: 'github' | 'discord' | 'kakao'
  name: string
  meta: string
}) {
  return (
    <Link href={href} rel="noopener noreferrer" target="_blank">
      <Flex
        _hover={{
          borderColor: '$vespertidePrimary',
          transform: 'translateX(2px)',
        }}
        alignItems="center"
        bg="$containerBackground"
        border="1px solid $border"
        borderRadius="$borderRadiusRadius08"
        gap="14px"
        px="16px"
        py="14px"
        transition="all .15s"
      >
        <Flex
          alignItems="center"
          boxSize="28px"
          justifyContent="center"
        >
          <MaskIcon color="$vespertidePrimary" icon={icon} size="24px" />
        </Flex>
        <Text color="$title" typography="body">
          {name}
        </Text>
        <Text
          color="$caption"
          fontFamily="D2Coding"
          fontSize="11px"
          ml="auto"
        >
          {meta}
        </Text>
      </Flex>
    </Link>
  )
}

function CommunitySection() {
  return (
    <Box
      as="section"
      bg="$vespertideBg"
      borderTop="1px solid $border"
      id="community"
      px={['20px', null, null, '40px']}
      py={['64px', null, null, '100px']}
    >
      <Box maxW="1240px" mx="auto">
        <Box
          bg="$containerBackground"
          border="1px solid $border"
          borderRadius="$borderRadiusRadius12"
          id="quickstart"
          overflow="hidden"
          pos="relative"
        >
          <Box
            bg="radial-gradient(ellipse 50% 80% at 100% 0%, rgba(14,195,92,0.12), transparent 70%)"
            inset="0"
            pointerEvents="none"
            pos="absolute"
          />
          <Flex
            alignItems={[null, null, null, 'flex-end']}
            flexDir={['column', null, null, 'row']}
            gap="40px"
            p={['40px 28px', null, null, '64px 48px']}
            pos="relative"
          >
            <VStack alignItems="flex-start" flex="1.4" w="100%">
              <Text
                color="$vespertidePrimary"
                fontFamily="D2Coding"
                fontSize="11px"
                letterSpacing="0.14em"
                mb="16px"
                textTransform="uppercase"
              >
                — Get started
              </Text>
              <Text color="$title" mb="16px" typography="h2">
                Install once.{' '}
                <Text as="span" color="$vespertidePrimary" fontStyle="italic">
                  Iterate forever.
                </Text>
              </Text>
              <Text color="$textSub" mb="24px" maxW="460px" typography="body">
                Vespertide is open source under Apache-2.0 and built in public.
                Join the community, file an issue, or pair with us in Discord.
              </Text>
              <CopyInstall />
              <Flex flexWrap="wrap" gap="12px" mt="18px">
                <Link href={DOCS_URL}>
                  <Button>Read the docs</Button>
                </Link>
                <Link
                  href={GITHUB_URL}
                  rel="noopener noreferrer"
                  target="_blank"
                >
                  <Flex
                    _hover={{ borderColor: '$caption' }}
                    alignItems="center"
                    as="button"
                    bg="transparent"
                    border="1px solid $border"
                    borderRadius="100px"
                    cursor="pointer"
                    gap="8px"
                    px={['28px', null, null, null, '$spacingSpacing32']}
                    py={['10px', null, null, null, '$spacingSpacing12']}
                    transition="border-color .15s"
                  >
                    <MaskIcon color="$title" icon="github" size="18px" />
                    <Text color="$title" typography="bodyLgEb">
                      Star on GitHub
                    </Text>
                  </Flex>
                </Link>
              </Flex>
            </VStack>
            <VStack flex="1" gap="10px" w="100%">
              <ChannelRow
                href={GITHUB_URL}
                icon="github"
                meta="issues · PRs"
                name="GitHub"
              />
              <ChannelRow
                href={DISCORD_URL}
                icon="discord"
                meta="English"
                name="Discord"
              />
              <ChannelRow
                href={KAKAO_URL}
                icon="kakao"
                meta="한국어"
                name="KakaoTalk 오픈채팅"
              />
            </VStack>
          </Flex>
        </Box>

        <Flex
          color="$caption"
          flexWrap="wrap"
          fontFamily="D2Coding"
          fontSize="11px"
          gap="20px"
          justifyContent="space-between"
          mt="40px"
          px="4px"
        >
          <Text color="$caption" fontFamily="D2Coding" fontSize="11px">
            Apache-2.0 · v{VERSION} ·{' '}
            <Link
              href={CRATES_URL}
              rel="noopener noreferrer"
              target="_blank"
            >
              crates.io
            </Link>
          </Text>
          <Text color="$caption" fontFamily="D2Coding" fontSize="11px">
            built with Rust · maintained in Seoul
          </Text>
        </Flex>
      </Box>
    </Box>
  )
}

export default async function HomePage() {
  const [heroHtml, modelHtml, cliHtml, runtimeHtml, exportHtml] =
    await Promise.all([
      highlight(HERO_MODEL_JSON, 'json'),
      highlight(EXAMPLE_MODEL, 'json'),
      highlight(EXAMPLE_CLI, 'shell'),
      highlight(EXAMPLE_RUNTIME, 'rust'),
      highlight(EXAMPLE_EXPORT, 'shell'),
    ])

  const examples: CodeExampleQuad = [
    { key: 'model', label: 'Model', file: 'models/post.json', html: modelHtml },
    { key: 'cli', label: 'CLI', file: '~/projects/blog', html: cliHtml },
    { key: 'runtime', label: 'Runtime', file: 'src/main.rs', html: runtimeHtml },
    {
      key: 'export',
      label: 'ORM export',
      file: '$ vespertide export',
      html: exportHtml,
    },
  ]

  return (
    <Box bg="$base" color="$title">
      <HeroSection codeHtml={heroHtml} />
      <FeaturesSection />
      <PipelineSection />
      <ExamplesSection examples={examples} />
      <CompatibilitySection />
      <CommunitySection />
    </Box>
  )
}
