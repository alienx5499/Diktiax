import React from 'react';

interface ErrorPanelProps {
  title: string;
  message: string;
  onRetry: () => void;
}

export function ErrorPanel({ title, message, onRetry }: ErrorPanelProps) {
  return (
    <div
      role="alert"
      className="w-full bg-[#FFF5F5] border-2 lg:border-4 border-[#D02020] shadow-[6px_6px_0px_0px_#121212] rounded-none p-6 flex flex-col sm:flex-row sm:items-center justify-between gap-4"
    >
      <div className="space-y-1">
        <p className="text-sm font-black uppercase text-[#D02020]">{title}</p>
        <p className="text-xs font-mono font-bold text-[#121212] break-words">{message}</p>
      </div>
      <button
        type="button"
        onClick={onRetry}
        className="px-4 py-2 rounded-none text-xs font-black uppercase bg-white hover:bg-[#F0C020] border-2 border-[#121212] shadow-[3px_3px_0px_0px_#121212] active:translate-x-[2px] active:translate-y-[2px] active:shadow-none cursor-pointer self-start sm:self-auto"
      >
        Retry
      </button>
    </div>
  );
}
