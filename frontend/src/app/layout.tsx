import type { Metadata } from 'next';
import { Outfit } from 'next/font/google';
import './globals.css';

const outfit = Outfit({
  variable: '--font-outfit',
  subsets: ['latin'],
  weight: ['400', '500', '700', '900'],
});

const DESCRIPTION = 'An AI agent that finds insurance fraud rings by walking a FalkorDB graph, then acts on what it finds.';

export const metadata: Metadata = {
  metadataBase: new URL('https://diktiax.vercel.app'),
  title: 'Diktiax | Graph AI Fraud Investigator',
  description: DESCRIPTION,
  openGraph: {
    title: 'Diktiax: the graph picks the next query',
    description: DESCRIPTION,
    url: '/',
    siteName: 'Diktiax',
    images: [{ url: '/og.jpg', width: 1200, height: 630, alt: 'Diktiax: a fraud ring drawn as a graph' }],
    type: 'website',
  },
  twitter: {
    card: 'summary_large_image',
    title: 'Diktiax: the graph picks the next query',
    description: DESCRIPTION,
    images: ['/og.jpg'],
  },
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en" className={`${outfit.variable} h-full antialiased`}>
      <body className="min-h-full flex flex-col bg-[#F0F0F0] text-[#121212] font-sans selection:bg-[#F0C020] selection:text-[#121212]">
        {children}
      </body>
    </html>
  );
}
