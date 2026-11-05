"use client";

import { useEffect, useRef } from "react";
import Highcharts from "highcharts";
import "highcharts/modules/accessibility";
import { useReducedMotion } from "motion/react";
import type { Finding } from "@/lib/types";
import { isAlert } from "@/lib/types";

export default function ActivityChart({ findings, hours, now }: { findings: Finding[]; hours: number; now: number }) {
  const container = useRef<HTMLDivElement>(null);
  const reduced = useReducedMotion();
  useEffect(() => {
    if (!container.current) return;
    const step = 3600_000;
    const end = Math.floor(now / step) * step;
    const start = end - (hours - 1) * step;
    const total = Array.from({ length: hours }, (_, i) => [start + i * step, 0]);
    const alerts = total.map(([time]) => [time, 0]);
    for (const finding of findings) {
      const index = Math.floor((new Date(finding.observed_at).getTime() - start) / step);
      if (index >= 0 && index < hours) { total[index][1]++; if (isAlert(finding)) alerts[index][1]++; }
    }
    const chart = Highcharts.chart(container.current, {
      chart: { type: "areaspline", backgroundColor: "transparent", height: 248, spacing: [18, 12, 8, 0],
        style: { fontFamily: "inherit" }, animation: !reduced },
      title: { text: undefined }, credits: { enabled: false }, legend: { enabled: false },
      time: { timezone: Intl.DateTimeFormat().resolvedOptions().timeZone },
      accessibility: { description: `Loaded incidents by observation hour over the last ${hours} hours. This is a bounded snapshot, not total chain activity.` },
      xAxis: { type: "datetime", tickInterval: hours <= 6 ? step : 4 * step,
        lineColor: "#e9ede9", tickLength: 0, labels: { style: { color: "#87918a", fontSize: "11px" }, format: "{value:%H:%M}" } },
      yAxis: { title: { text: undefined }, min: 0, allowDecimals: false, gridLineColor: "#edf0ec", gridLineDashStyle: "Dash",
        labels: { style: { color: "#87918a", fontSize: "11px" } } },
      tooltip: { shared: true, backgroundColor: "#fff", borderColor: "#e1e7df", borderRadius: 10, shadow: false, xDateFormat: "%b %e, %H:%M" },
      plotOptions: { series: { animation: !reduced, marker: { enabled: false }, states: { inactive: { opacity: 1 } } }, areaspline: { lineWidth: 2.5, fillOpacity: 0.08 } },
      series: [
        { type: "areaspline", name: "Loaded incidents", data: total, color: "#3a8067", fillColor: { linearGradient: { x1: 0, y1: 0, x2: 0, y2: 1 }, stops: [[0, "rgba(87,151,116,0.22)"], [1, "rgba(87,151,116,0.01)"]] } },
        { type: "areaspline", name: "Active high / critical", data: alerts, color: "#cd8b4b", fillOpacity: 0, lineWidth: 2, dashStyle: "ShortDash" },
      ],
    });
    return () => chart.destroy();
  }, [findings, hours, now, reduced]);
  return <div ref={container} className="activity-chart" aria-label="Loaded incident activity chart" />;
}
