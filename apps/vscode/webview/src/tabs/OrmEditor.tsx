import React, { useCallback, useRef } from 'react';
import { postMessage } from '../vscode';
import type { OrmType } from '../vscode';
import type { AppState } from '../App';

type Props = {
  state: AppState;
  setState: React.Dispatch<React.SetStateAction<AppState>>;
};

const ORM_TYPES: OrmType[] = ['prisma', 'typeorm', 'drizzle', 'jpa', 'sqlalchemy', 'gorm'];

const PLACEHOLDER: Record<OrmType, string> = {
  prisma: `model User {\n  id    Int    @id @default(autoincrement())\n  name  String\n  posts Post[]\n}\n\nmodel Post {\n  id     Int  @id @default(autoincrement())\n  userId Int\n  user   User @relation(fields: [userId], references: [id])\n}`,
  typeorm: `@Entity()\nexport class User {\n  @PrimaryGeneratedColumn() id: number;\n  @Column() name: string;\n}`,
  drizzle: `export const users = pgTable('users', {\n  id: serial('id').primaryKey(),\n  name: text('name').notNull(),\n});`,
  jpa: `@Entity\npublic class User {\n  @Id @GeneratedValue\n  private Long id;\n  private String name;\n}`,
  sqlalchemy: `class User(Base):\n  __tablename__ = 'users'\n  id = Column(Integer, primary_key=True)\n  name = Column(String, nullable=False)`,
  gorm: `type User struct {\n  gorm.Model\n  Name string\n}`,
};

export default function OrmEditor({ state, setState }: Props) {
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const triggerParse = useCallback(
    (source: string, orm: OrmType) => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
      debounceRef.current = setTimeout(() => {
        postMessage({ type: 'parse_orm', source, orm });
      }, 300);
    },
    []
  );

  const handleChange = (source: string) => {
    setState((prev) => ({ ...prev, ormSource: source }));
    triggerParse(source, state.ormType);
  };

  const handleOrmType = (orm: OrmType) => {
    setState((prev) => ({ ...prev, ormType: orm }));
    if (state.ormSource) triggerParse(state.ormSource, orm);
  };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* ORM selector */}
      <div
        style={{
          display: 'flex',
          gap: 4,
          padding: '6px 8px',
          background: 'var(--vscode-editorWidget-background, #252526)',
          borderBottom: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
          flexShrink: 0,
          flexWrap: 'wrap',
        }}
      >
        {ORM_TYPES.map((orm) => (
          <button
            key={orm}
            onClick={() => handleOrmType(orm)}
            style={{
              padding: '2px 10px',
              border: '1px solid',
              borderColor:
                state.ormType === orm
                  ? 'var(--vscode-focusBorder, #007acc)'
                  : 'var(--vscode-input-border, rgba(255,255,255,0.2))',
              borderRadius: 3,
              background:
                state.ormType === orm
                  ? 'var(--vscode-button-background, #0e639c)'
                  : 'transparent',
              color:
                state.ormType === orm
                  ? 'var(--vscode-button-foreground, #fff)'
                  : 'var(--vscode-foreground)',
              fontSize: 11,
              transition: 'all 0.1s',
            }}
          >
            {orm}
          </button>
        ))}
      </div>

      {/* Split: editor | ERD */}
      <div style={{ display: 'flex', flex: 1, overflow: 'hidden' }}>
        {/* Code textarea */}
        <textarea
          value={state.ormSource}
          onChange={(e) => handleChange(e.target.value)}
          placeholder={PLACEHOLDER[state.ormType]}
          spellCheck={false}
          style={{
            flex: 1,
            resize: 'none',
            border: 'none',
            outline: 'none',
            padding: '12px',
            fontFamily:
              'var(--vscode-editor-font-family, "Fira Code", Consolas, "Courier New", monospace)',
            fontSize: 'var(--vscode-editor-font-size, 13px)',
            color: 'var(--vscode-editor-foreground, #d4d4d4)',
            background: 'var(--vscode-editor-background, #1e1e1e)',
            lineHeight: 1.5,
            tabSize: 2,
            overflowY: 'auto',
          }}
        />

        {/* Divider */}
        <div
          style={{
            width: 1,
            background: 'var(--vscode-panel-border, rgba(255,255,255,0.1))',
            flexShrink: 0,
          }}
        />

        {/* ERD panel */}
        <div
          style={{
            flex: 1,
            overflow: 'auto',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            background: 'var(--vscode-editor-background, #1e1e1e)',
            padding: 12,
          }}
        >
          {state.svg ? (
            <div
              dangerouslySetInnerHTML={{ __html: state.svg }}
              style={{ maxWidth: '100%', maxHeight: '100%' }}
            />
          ) : (
            <div style={{ opacity: 0.35, fontSize: 12, textAlign: 'center', lineHeight: 1.8 }}>
              <div>ORM 코드를 입력하면</div>
              <div>ERD가 여기에 표시됩니다</div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
