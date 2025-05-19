#!/usr/bin/env bash
set -euo pipefail

# File with lines like:
# avg: 19.83, p95: 22.84, p99: 23.76, min: 3.01, max: 26.36, median: 20.86
INPUT_FILE="${1:-metrics.txt}"

# Initialize sums
sum_avg=0
sum_p95=0
sum_p99=0
sum_max=0
sum_min=0
sum_median=0
count=0

# Read and accumulate
while read -r line; do
    avg=$(echo "$line" | grep -oP 'avg:\s*\K[\d\.]+')
    p95=$(echo "$line" | grep -oP 'p95:\s*\K[\d\.]+')
    p99=$(echo "$line" | grep -oP 'p99:\s*\K[\d\.]+')
    max=$(echo "$line" | grep -oP 'max:\s*\K[\d\.]+')
    min=$(echo "$line" | grep -oP 'min:\s*\K[\d\.]+')
    median=$(echo "$line" | grep -oP 'median:\s*\K[\d\.]+')

    sum_avg=$(awk -v a="$sum_avg" -v b="$avg" 'BEGIN { print a + b }')
    sum_p95=$(awk -v a="$sum_p95" -v b="$p95" 'BEGIN { print a + b }')
    sum_p99=$(awk -v a="$sum_p99" -v b="$p99" 'BEGIN { print a + b }')
    sum_max=$(awk -v a="$sum_max" -v b="$max" 'BEGIN { print a + b }')
    sum_min=$(awk -v a="$sum_min" -v b="$min" 'BEGIN { print a + b }')
    sum_median=$(awk -v a="$sum_median" -v b="$median" 'BEGIN { print a + b }')

    count=$((count + 1))
done < "$INPUT_FILE"

# Compute averages
avg_avg=$(awk -v s="$sum_avg" -v c="$count" 'BEGIN { printf "%.3f", s / c }')
avg_p95=$(awk -v s="$sum_p95" -v c="$count" 'BEGIN { printf "%.3f", s / c }')
avg_p99=$(awk -v s="$sum_p99" -v c="$count" 'BEGIN { printf "%.3f", s / c }')
avg_max=$(awk -v s="$sum_max" -v c="$count" 'BEGIN { printf "%.3f", s / c }')
avg_min=$(awk -v s="$sum_min" -v c="$count" 'BEGIN { printf "%.3f", s / c }')
avg_median=$(awk -v s="$sum_median" -v c="$count" 'BEGIN { printf "%.3f", s / c }')

# Output results
echo "Average of Metrics across $count runs:"
echo "avg     = $avg_avg ms"
echo "p95     = $avg_p95 ms"
echo "p99     = $avg_p99 ms"
echo "median  = $avg_median ms"
echo "min     = $avg_min ms"
echo "max     = $avg_max ms"
