import { Logo } from './Logo';

export function Footer() {
  return (
    <footer className="border-t border-[var(--color-border)] mt-auto">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
        <div className="flex flex-col sm:flex-row items-center justify-between gap-4">
          <Logo />
          <p className="text-sm text-[var(--color-text-secondary)]">
            Ship Rust. Write TypeScript.
          </p>
        </div>
      </div>
    </footer>
  );
}
