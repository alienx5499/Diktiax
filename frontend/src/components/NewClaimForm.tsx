'use client';

import React, { useState } from 'react';
import { ClaimSummary, NewClaim, createClaim } from '@/lib/api';

interface NewClaimFormProps {
  onCreated: (claim: ClaimSummary) => Promise<void>;
}

type Field = { name: keyof NewClaim; label: string; required: boolean };

const FIELDS: Field[] = [
  { name: 'claim_id', label: 'Claim ID', required: false },
  { name: 'claimant_name', label: 'Claimant name', required: true },
  { name: 'phone', label: 'Phone', required: true },
  { name: 'address', label: 'Address', required: true },
  { name: 'vehicle_vin', label: 'Vehicle VIN', required: true },
  { name: 'location_id', label: 'Location ID', required: true },
  { name: 'repair_shop_id', label: 'Repair shop ID', required: true },
  { name: 'clinic_id', label: 'Clinic ID', required: false },
  { name: 'adjuster_id', label: 'Adjuster ID', required: false },
];

const inputCls =
  'w-full px-3 py-2 rounded-none bg-white border-2 border-[#121212] font-mono text-xs focus:outline-none focus:bg-[#FFFDF0] focus:shadow-[3px_3px_0px_0px_#1040C0]';

function FieldError({ id, message }: { id: string; message?: string }) {
  return message ? <span id={id} className="block text-[10px] font-mono font-bold normal-case text-[#D02020]">{message}</span> : null;
}

export function NewClaimForm({ onCreated }: NewClaimFormProps) {
  const [values, setValues] = useState<Record<string, string>>({});
  const [policeReport, setPoliceReport] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    const get = (k: string) => (values[k] ?? '').trim();
    const errors: Record<string, string> = {};
    for (const f of FIELDS) if (f.required && !get(f.name)) errors[f.name] = `${f.label} is required`;
    const amount = Number(get('amount'));
    if (!get('amount') || !Number.isFinite(amount) || amount <= 0) {
      errors.amount = 'Amount must be a number greater than 0';
    }
    setFieldErrors(errors);
    const messages = Object.values(errors);
    if (messages.length > 0) return setError(messages.join('; '));
    const claim: NewClaim = {
      claimant_name: get('claimant_name'),
      phone: get('phone'),
      address: get('address'),
      vehicle_vin: get('vehicle_vin'),
      location_id: get('location_id'),
      repair_shop_id: get('repair_shop_id'),
      amount,
      police_report: policeReport,
    };
    for (const k of ['claim_id', 'clinic_id', 'adjuster_id'] as const) {
      if (get(k)) claim[k] = get(k);
    }
    setPending(true);
    setError(null);
    try {
      await onCreated(await createClaim(claim));
      setValues({});
      setFieldErrors({});
      setPoliceReport(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setPending(false);
    }
  };

  return (
    <form
      onSubmit={submit}
      noValidate
      aria-busy={pending}
      className="w-full bg-white border-2 lg:border-4 border-[#121212] shadow-[6px_6px_0px_0px_#121212] rounded-none p-6 space-y-4"
    >
      <h3 className="text-base font-black uppercase tracking-tight">Submit New Claim</h3>
      <fieldset disabled={pending} className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
        {FIELDS.map((f) => (
          <label key={f.name} className="space-y-1 text-[11px] font-black uppercase tracking-wider">
            <span>{f.label}{f.required ? ' *' : ' (optional)'}</span>
            <input
              className={inputCls}
              value={values[f.name] ?? ''}
              required={f.required}
              aria-invalid={Boolean(fieldErrors[f.name])}
              aria-describedby={fieldErrors[f.name] ? `${f.name}-error` : undefined}
              onChange={(e) => setValues({ ...values, [f.name]: e.target.value })}
            />
            <FieldError id={`${f.name}-error`} message={fieldErrors[f.name]} />
          </label>
        ))}
        <label className="space-y-1 text-[11px] font-black uppercase tracking-wider">
          <span>Amount (USD) *</span>
          <input
            className={inputCls}
            type="number"
            min="0"
            step="any"
            required
            aria-invalid={Boolean(fieldErrors.amount)}
            aria-describedby={fieldErrors.amount ? 'amount-error' : undefined}
            value={values.amount ?? ''}
            onChange={(e) => setValues({ ...values, amount: e.target.value })}
          />
          <FieldError id="amount-error" message={fieldErrors.amount} />
        </label>
        <label className="flex items-center gap-2 text-[11px] font-black uppercase tracking-wider self-end pb-2">
          <input type="checkbox" checked={policeReport} onChange={(e) => setPoliceReport(e.target.checked)} className="w-4 h-4 accent-[#D02020]" />
          <span>Police report filed</span>
        </label>
      </fieldset>
      {error && (
        <p role="alert" className="text-xs font-mono font-bold text-[#D02020] break-words">{error}</p>
      )}
      <button
        type="submit"
        disabled={pending}
        className="h-11 px-6 rounded-none text-xs font-black uppercase tracking-wider text-white bg-[#D02020] border-2 border-[#121212] shadow-[4px_4px_0px_0px_#121212] active:translate-x-[2px] active:translate-y-[2px] active:shadow-none disabled:opacity-50 cursor-pointer"
      >
        {pending ? 'Submitting...' : 'Submit Claim'}
      </button>
    </form>
  );
}
