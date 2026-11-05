"use client";

import { useEffect, useRef } from "react";
import { X, AlertCircle, SearchX, LoaderCircle } from "lucide-react";
import { motion } from "motion/react";
import { CHAINS, chainName, humanize } from "@/lib/types";

export function ChainIcon({ id, small = false }: { id: number; small?: boolean }) {
  return <span className={`chain-icon ${small ? "small" : ""}`} style={{ "--chain-color": CHAINS[id]?.color ?? "#6b7973" } as React.CSSProperties} aria-hidden="true">{id === 1 ? "♦" : id === 8453 ? "━" : id === 56 ? "⟐" : id === 42161 ? "▰" : "•"}</span>;
}

export function ChainLabel({ id }: { id: number }) { return <span className="chain-label"><ChainIcon id={id} small />{chainName(id)}</span>; }

export function Severity({ value }: { value: string }) { return <span className={`badge severity-${value}`}><i />{humanize(value)}</span>; }

export function ErrorBox({ error, retry }: { error: Error; retry?: () => void }) {
  return <div className="error-box" role="alert"><AlertCircle size={18} /><div><strong>Unable to load monitor data</strong><p>{error.message}</p></div>{retry && <button className="button small-button" onClick={retry}>Retry</button>}</div>;
}

export function Empty({ title = "No findings in this view", detail = "Try another filter, or wait for the monitor to observe new activity." }: { title?: string; detail?: string }) {
  return <div className="empty-state"><SearchX size={28} /><h3>{title}</h3><p>{detail}</p></div>;
}

export function Loading({ label = "Loading monitor data…" }: { label?: string }) { return <div className="loading-state" role="status"><LoaderCircle className="spin" size={20} />{label}</div>; }

export function Modal({ title, children, onClose, drawer = false }: { title: string; children: React.ReactNode; onClose: () => void; drawer?: boolean }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    dialog?.showModal();
    const before = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => { dialog?.close(); document.body.style.overflow = before; };
  }, []);
  return <dialog ref={ref} className={drawer ? "modal drawer" : "modal"} aria-labelledby="modal-title"
    onCancel={onClose} onClick={(event) => { if (event.target === event.currentTarget) onClose(); }}>
    <motion.div className="modal-inner" initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.2 }}>
      <div className="modal-heading"><div><span className="eyebrow">CHAIN SIREN</span><h2 id="modal-title">{title}</h2></div><button onClick={onClose} className="icon-button" aria-label="Close dialog"><X size={20} /></button></div>
      {children}
    </motion.div>
  </dialog>;
}
