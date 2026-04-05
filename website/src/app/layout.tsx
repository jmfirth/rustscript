import type { Metadata } from 'next';
import { ThemeProvider } from 'next-themes';
import { Header } from '@/components/Header';
import { Footer } from '@/components/Footer';
import '@/styles/globals.css';

export const metadata: Metadata = {
  title: 'RustScript - Write TypeScript. Ship Rust.',
  description:
    'A TypeScript-native authoring language that compiles to idiomatic Rust. 3MB binaries. No V8. No garbage collector.',
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en" suppressHydrationWarning>
      <body className="min-h-screen flex flex-col">
        <ThemeProvider attribute="class" defaultTheme="system" enableSystem>
          <Header />
          <main className="flex-1">{children}</main>
          <Footer />
        </ThemeProvider>
      </body>
    </html>
  );
}
