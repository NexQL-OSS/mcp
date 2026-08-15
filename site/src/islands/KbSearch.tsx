import { useMemo, useState } from "preact/hooks";
import { kbArticles, kbCategories, type KbArticle } from "../data/kb";

export function KbSearch() {
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState<string | null>(null);

  const results = useMemo(() => {
    const term = query.trim().toLowerCase();

    return kbArticles.filter((article) => {
      if (category && article.category !== category) return false;
      if (!term) return true;

      const haystack = [
        article.title,
        article.summary,
        article.body,
        article.category,
        article.tags.join(" "),
      ]
        .join(" ")
        .toLowerCase();

      return haystack.includes(term);
    });
  }, [query, category]);

  return (
    <div>
      <input
        class="kb-search"
        type="search"
        placeholder="Search: access mode, docker, index depth, glibc…"
        value={query}
        onInput={(e) => setQuery((e.target as HTMLInputElement).value)}
        aria-label="Search the knowledge base"
      />

      <div class="nx-agent-chips">
        <button
          type="button"
          class={category === null ? "nx-agent-chip is-active" : "nx-agent-chip"}
          aria-pressed={category === null ? "true" : "false"}
          onClick={() => setCategory(null)}
        >
          All
        </button>
        {kbCategories.map((c) => (
          <button
            key={c}
            type="button"
            class={category === c ? "nx-agent-chip is-active" : "nx-agent-chip"}
            aria-pressed={category === c ? "true" : "false"}
            onClick={() => setCategory(category === c ? null : c)}
          >
            {c}
          </button>
        ))}
      </div>

      <p class="kb-count">
        {results.length} of {kbArticles.length} articles
      </p>

      <div class="kb-results" aria-live="polite">
        {results.length === 0 ? (
          <p class="kb-empty">
            Nothing matches “{query}”. Try <em>access mode</em>, <em>index</em>, or{" "}
            <em>install</em>.
          </p>
        ) : (
          results.map((article) => <KbCard key={article.id} article={article} />)
        )}
      </div>
    </div>
  );
}

function KbCard({ article }: { article: KbArticle }) {
  return (
    <article class="kb-card" id={`kb-${article.id}`}>
      <div class="kb-card-meta">
        <span class="kb-tag">{article.category}</span>
        {article.tags.slice(0, 3).map((tag) => (
          <span class="kb-tag kb-tag-faint" key={tag}>
            {tag}
          </span>
        ))}
      </div>
      <h3>{article.title}</h3>
      <p class="kb-card-summary">{article.summary}</p>
      <p class="kb-card-body">{article.body}</p>
    </article>
  );
}
