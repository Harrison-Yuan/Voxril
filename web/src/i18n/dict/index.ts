/**
 * 按模块拆分的文案分片。
 *
 * 每片同时给出 zh 与 en 两份，逐键并排，从结构上避免「加了中文忘了英文」。
 * 主字典 zh.ts / en.ts 把这些分片 spread 进去，`MessageKey` 仍是精确的字面量联合。
 *
 * 约定：
 * - 键前缀用分片所属模块（见各文件顶部注释），不要与其它分片重名
 * - 已存在的键直接复用 zh.ts 里的，不要在同名键上重复定义（spread 会静默覆盖既有文案）
 */
export { train } from './train'
export { trainSteps } from './trainSteps'
export { models } from './models'
export { cover } from './cover'
export { infer } from './infer'
export { settings } from './settings'
export { shell } from './shell'
