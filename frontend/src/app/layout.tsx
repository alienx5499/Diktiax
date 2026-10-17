import type { Metadata } from 'next';
import { Outfit } from 'next/font/google';
import './globals.css';

const outfit = Outfit({
  variable: '--font-outfit',
  subsets: ['latin'],
  weight: ['400', '500', '700', '900'],
});

export const metadata: Metadata = {
  title: 'Diktiax | Autonomous Graph AI Fraud Agent',
  description:
    'Autonomous Graph AI agent for high-velocity insurance fraud detection and multi-hop collision ring resolution using FalkorDB and Rust.',
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
