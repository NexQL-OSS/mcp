import { useMemo, useState } from "preact/hooks";
import { kbArticles, type KbArticle } from "../data/kb";

export function KbSearch() {
  const [query, setQuery] = useState("");

  const results = useMemo(() => {
    const term = query.trim().toLowerCase();
    if (!term) return kbArticles;
    return kbArticles.filter((article) => {
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
  }, [query]);

  return (
    <div>
      <input
        class="kb-search"
        type="search"
        placeholder="Search: schema index, docker, access mode, pii…"
        value={query}
        onInput={(e) => setQuery((e.target as HTMLInputElement).value)}
        aria-label="Search knowledge base"
      />
      <div class="kb-results" aria-live="polite">
        {results.length === 0 ? (
          <p class="kb-empty">No articles match. Try schema, install, or access mode.</p>
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
          <span class="kb-tag kb-tag-faint" key={tag}>{tag}</span>
        ))}
      </div>
      <h3>{article.title}</h3>
      <p>{article.summary}</p>
      <p class="kb-body">{article.body}</p>
    </article>
  );
}
