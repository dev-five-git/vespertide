import React, { useState, useEffect } from 'react';
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

export type AppState = {
  ormSource: string;
  ormType: OrmType;
  svg: string;
  schema: Schema;
  postgres: string;
  mysql: string;
  sqlite: string;
  error: string | null;
  theme: 'dark' | 'light';
};

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
    <div data-theme={state.theme} style={{ display: 'flex', flexDirection: 'column', height: '100vh' }}>
      <div
        role="tablist"
        style={{
          display: 'flex',
          borderBottom: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
          background: 'var(--vscode-editorGroupHeader-tabsBackground, #2d2d2d)',
          flexShrink: 0,
        }}
      >
        {TABS.map(({ id, label }) => (
          <button
            key={id}
            role="tab"
            aria-selected={tab === id}
            onClick={() => setTab(id)}
            style={{
              flex: 1,
              padding: '8px 4px',
              border: 'none',
              borderBottom: tab === id
                ? '2px solid var(--vscode-focusBorder, #007acc)'
                : '2px solid transparent',
              background: 'transparent',
              color: tab === id
                ? 'var(--vscode-foreground)'
                : 'var(--vscode-tab-inactiveForeground, #8e8e8e)',
              fontSize: '11px',
              fontWeight: tab === id ? 600 : 400,
              cursor: 'pointer',
              transition: 'color 0.1s, border-color 0.1s',
            }}
          >
            {label}
          </button>
        ))}
      </div>

      {state.error && (
        <div style={{
          padding: '6px 12px',
          background: 'var(--vscode-inputValidation-errorBackground, rgba(90,29,29,0.9))',
          color: 'var(--vscode-inputValidation-errorForeground, #f48771)',
          fontSize: '12px',
          flexShrink: 0,
        }}>
          {state.error}
        </div>
      )}

      <div style={{ flex: 1, overflow: 'hidden' }}>
        {tab === 'editor'    && <OrmEditor    state={state} setState={setState} />}
        {tab === 'migration' && <MigrationDiff state={state} setState={setState} />}
        {tab === 'export'    && <Export       state={state} setState={setState} />}
      </div>
    </div>
  );
}
