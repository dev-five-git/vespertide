import React, { useState, useEffect } from 'react';
import { Box, Flex, VStack } from '@devup-ui/react';
import { onMessage } from './vscode';
import type { HostMessage, OrmType, Schema } from './vscode';
import OrmEditor from './tabs/OrmEditor';
import MigrationDiff from './tabs/MigrationDiff';
import Export from './tabs/Export';

type Tab = 'editor' | 'migration' | 'export';

export const DEFAULT_SCHEMAS: Record<OrmType, string> = {
  prisma: `model User {
  id        Int      @id @default(autoincrement())
  email     String   @unique
  name      String?
  createdAt DateTime @default(now())
  posts     Post[]
  profile   Profile?
}

model Profile {
  id     Int    @id @default(autoincrement())
  bio    String
  userId Int    @unique
  user   User   @relation(fields: [userId], references: [id])
}

model Post {
  id         Int        @id @default(autoincrement())
  title      String
  content    String?
  published  Boolean    @default(false)
  createdAt  DateTime   @default(now())
  authorId   Int
  author     User       @relation(fields: [authorId], references: [id])
  tags       TagOnPost[]
}

model Tag {
  id    Int        @id @default(autoincrement())
  name  String     @unique
  posts TagOnPost[]
}

model TagOnPost {
  postId Int
  tagId  Int
  post   Post @relation(fields: [postId], references: [id])
  tag    Tag  @relation(fields: [tagId], references: [id])
  @@id([postId, tagId])
}`,

  typeorm: `@Entity()
export class User {
  @PrimaryGeneratedColumn() id: number;
  @Column({ unique: true }) email: string;
  @Column({ nullable: true }) name: string;
  @OneToMany(() => Post, post => post.author) posts: Post[];
}

@Entity()
export class Post {
  @PrimaryGeneratedColumn() id: number;
  @Column() title: string;
  @Column({ nullable: true }) content: string;
  @Column({ default: false }) published: boolean;
  @ManyToOne(() => User, user => user.posts) author: User;
}`,

  drizzle: `export const users = pgTable('users', {
  id:        serial('id').primaryKey(),
  email:     text('email').notNull().unique(),
  name:      text('name'),
  createdAt: timestamp('created_at').defaultNow(),
});

export const posts = pgTable('posts', {
  id:        serial('id').primaryKey(),
  title:     text('title').notNull(),
  content:   text('content'),
  published: boolean('published').default(false),
  authorId:  integer('author_id').references(() => users.id),
});`,

  jpa: `@Entity
public class User {
  @Id @GeneratedValue(strategy = GenerationType.IDENTITY)
  private Long id;
  @Column(unique = true) private String email;
  private String name;
  @OneToMany(mappedBy = "author") private List<Post> posts;
}

@Entity
public class Post {
  @Id @GeneratedValue(strategy = GenerationType.IDENTITY)
  private Long id;
  private String title;
  private String content;
  private Boolean published = false;
  @ManyToOne @JoinColumn(name = "author_id") private User author;
}`,

  sqlalchemy: `class User(Base):
  __tablename__ = 'users'
  id        = Column(Integer, primary_key=True)
  email     = Column(String, unique=True, nullable=False)
  name      = Column(String)
  posts     = relationship('Post', back_populates='author')

class Post(Base):
  __tablename__ = 'posts'
  id        = Column(Integer, primary_key=True)
  title     = Column(String, nullable=False)
  content   = Column(String)
  published = Column(Boolean, default=False)
  author_id = Column(Integer, ForeignKey('users.id'))
  author    = relationship('User', back_populates='posts')`,

  gorm: `type User struct {
  gorm.Model
  Email   string \`gorm:"uniqueIndex;not null"\`
  Name    string
  Posts   []Post
}

type Post struct {
  gorm.Model
  Title     string \`gorm:"not null"\`
  Content   string
  Published bool   \`gorm:"default:false"\`
  AuthorID  uint
  Author    User
}`,
};

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
