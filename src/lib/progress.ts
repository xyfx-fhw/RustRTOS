// src/lib/progress.ts

export interface ArticleProgress {
  status: 'completed' | 'in-progress' | 'not-started';
  sections: Record<string, boolean>;
}

interface ProgressStore {
  articles: Record<string, ArticleProgress>;
  blocks: Record<string, 'pass' | 'fail'>;  // 新增：blockId → latest
  exercises: Record<string, { completed: boolean; attempts: number }>;
  quizzes: Record<string, { score: number; completedAt: string }>;
  certificate: { name: string; earnedAt: string } | null;
}

const STORAGE_KEY = 'rust-rtos-progress';

function load(): ProgressStore {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return empty();
    const parsed = JSON.parse(raw) as Partial<ProgressStore>;
    // 兼容旧数据：补齐缺失字段
    return {
      articles: parsed.articles ?? {},
      blocks: parsed.blocks ?? {},
      exercises: parsed.exercises ?? {},
      quizzes: parsed.quizzes ?? {},
      certificate: parsed.certificate ?? null,
    };
  } catch {
    return empty();
  }
}

function empty(): ProgressStore {
  return { articles: {}, blocks: {}, exercises: {}, quizzes: {}, certificate: null };
}

function save(store: ProgressStore): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(store));
}

function deriveStatus(
  sections: Record<string, boolean>
): ArticleProgress['status'] {
  const vals = Object.values(sections);
  if (vals.length === 0) return 'not-started';
  if (vals.every(Boolean)) return 'completed';
  if (vals.some(Boolean)) return 'in-progress';
  return 'not-started';
}

// ── 文章进度 ─────────────────────────────────────────────

export function getArticleStatus(slug: string): ArticleProgress['status'] {
  return load().articles[slug]?.status ?? 'not-started';
}

export function getSectionStatus(
  articleSlug: string
): Record<string, boolean> {
  return load().articles[articleSlug]?.sections ?? {};
}

/** 页面加载时同步当前文章的节段列表：新节段初始化为 false，已删除的旧节段剪掉。 */
export function initArticleSections(
  articleSlug: string,
  sectionTitles: string[]
): void {
  const store = load();
  const ap: ArticleProgress = store.articles[articleSlug] ?? {
    status: 'not-started',
    sections: {},
  };

  // 重建 sections：只保留当前文章存在的节段，旧节段一并清除
  const newSections: Record<string, boolean> = {};
  for (const title of sectionTitles) {
    newSections[title] = ap.sections[title] ?? false;
  }

  const oldKeys = Object.keys(ap.sections).sort().join(',');
  const newKeys = Object.keys(newSections).sort().join(',');
  if (oldKeys !== newKeys) {
    ap.sections = newSections;
    ap.status = deriveStatus(ap.sections);
    store.articles[articleSlug] = ap;
    save(store);
  }
}

export function markSectionRead(
  articleSlug: string,
  sectionTitle: string
): void {
  const store = load();
  const ap: ArticleProgress = store.articles[articleSlug] ?? {
    status: 'not-started',
    sections: {},
  };
  ap.sections[sectionTitle] = true;
  ap.status = deriveStatus(ap.sections);
  store.articles[articleSlug] = ap;
  save(store);
  window.dispatchEvent(new CustomEvent('progress-updated'));
}

/**
 * 双向设置某 section 完成状态：practice tab 的 recompute 需要把绿变灰。
 * （markSectionRead 仅能 set true，不能 set false。）
 */
export function setSectionStatus(
  articleSlug: string,
  sectionTitle: string,
  completed: boolean
): void {
  const store = load();
  const ap: ArticleProgress = store.articles[articleSlug] ?? {
    status: 'not-started',
    sections: {},
  };
  const prev = ap.sections[sectionTitle] ?? false;
  if (prev === completed) return;  // 未变化不触发事件
  ap.sections[sectionTitle] = completed;
  ap.status = deriveStatus(ap.sections);
  store.articles[articleSlug] = ap;
  save(store);
  window.dispatchEvent(new CustomEvent('progress-updated'));
}

export function markArticleComplete(slug: string): void {
  const store = load();
  const ap: ArticleProgress = store.articles[slug] ?? {
    status: 'not-started',
    sections: {},
  };
  ap.status = 'completed';
  store.articles[slug] = ap;
  save(store);
  window.dispatchEvent(new CustomEvent('progress-updated'));
}

export function resetArticle(slug: string): void {
  const store = load();
  delete store.articles[slug];
  // 清理该文章所有 block latest
  for (const blockId of Object.keys(store.blocks)) {
    if (blockId.startsWith(`${slug}#`)) {
      delete store.blocks[blockId];
    }
  }
  save(store);
  window.dispatchEvent(new CustomEvent('progress-updated'));
}

export function resetChapter(chapterKey: string): void {
  const store = load();
  for (const slug of Object.keys(store.articles)) {
    if (slug.split('/')[0] === chapterKey) {
      delete store.articles[slug];
    }
  }
  for (const blockId of Object.keys(store.blocks)) {
    if (blockId.split('/')[0] === chapterKey) {
      delete store.blocks[blockId];
    }
  }
  save(store);
  window.dispatchEvent(new CustomEvent('progress-updated'));
}

export function resetAll(): void {
  const prev = load();
  const fresh = empty();
  // 重置进度时保留证书，避免用户重新填写姓名
  fresh.certificate = prev.certificate;
  save(fresh);
  window.dispatchEvent(new CustomEvent('progress-updated'));
}

// ── 练习 & 测验（子计划 5 使用）─────────────────────────

export function markExerciseComplete(id: string, attempts: number): void {
  const store = load();
  store.exercises[id] = { completed: true, attempts };
  save(store);
  window.dispatchEvent(new CustomEvent('progress-updated'));
}

export function saveQuizResult(slug: string, score: number): void {
  const store = load();
  store.quizzes[slug] = { score, completedAt: new Date().toISOString() };
  save(store);
  window.dispatchEvent(new CustomEvent('progress-updated'));
}

export function resetExercise(id: string): void {
  const store = load();
  delete store.exercises[id];
  save(store);
  window.dispatchEvent(new CustomEvent('progress-updated'));
}

// ── 互动块 latest（practice tab 完成判定用）──────────────

export function markBlockResult(
  blockId: string,
  result: 'pass' | 'fail'
): void {
  const store = load();
  store.blocks[blockId] = result;
  save(store);
  window.dispatchEvent(new CustomEvent('progress-updated'));
}

export function getBlockResult(blockId: string): 'pass' | 'fail' | null {
  return load().blocks[blockId] ?? null;
}

export function resetQuiz(slug: string): void {
  const store = load();
  delete store.quizzes[slug];
  save(store);
  window.dispatchEvent(new CustomEvent('progress-updated'));
}

// ── 进度计算 ─────────────────────────────────────────────

/**
 * 返回 0-100 的整体进度百分比，精确到小节粒度。
 * 每篇文章权重相等（1），有 H2 的文章按已读小节占比折算，无 H2 的文章完成即得满分。
 */
export function getOverallProgress(totalArticles: number): number {
  if (totalArticles === 0) return 0;
  const store = load();
  let completedWeight = 0;

  for (const ap of Object.values(store.articles)) {
    const sectionVals = Object.values(ap.sections);
    if (sectionVals.length > 0) {
      completedWeight += sectionVals.filter(Boolean).length / sectionVals.length;
    } else if (ap.status === 'completed') {
      completedWeight += 1;
    }
  }

  return Math.min(100, Math.round((completedWeight / totalArticles) * 100));
}

/**
 * 返回某章节的进度百分比。
 * articleSlugs：该章节所有文章（含 index）的 slug 列表。
 */
export function getChapterProgress(articleSlugs: string[]): number {
  if (articleSlugs.length === 0) return 0;
  const store = load();
  const completed = articleSlugs.filter(
    (s) => store.articles[s]?.status === 'completed'
  ).length;
  return Math.round((completed / articleSlugs.length) * 100);
}

// ── 证书 ─────────────────────────────────────────────────

export function saveCertificateName(name: string): void {
  const store = load();
  store.certificate = { name, earnedAt: new Date().toISOString() };
  save(store);
}

export function getCertificate(): { name: string; earnedAt: string } | null {
  return load().certificate;
}
