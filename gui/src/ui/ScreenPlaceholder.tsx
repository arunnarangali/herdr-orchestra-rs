/** What every screen shows until its milestone builds it. */
export function ScreenPlaceholder({ title }: { title: string }) {
  return (
    <section className="screen" aria-labelledby="screen-title">
      <h1 id="screen-title">{title}</h1>
      <p className="muted">Nothing here yet.</p>
    </section>
  );
}
