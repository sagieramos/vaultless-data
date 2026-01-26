"use client";

import { useState } from 'react';
import { Search, SlidersHorizontal, ArrowUpDown, X } from 'lucide-react';
import { Button } from '../ui/button';
import { Input } from '../ui/input';
import { Badge } from '../ui/badge';
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '../ui/popover';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../ui/select';
import { Checkbox } from '../ui/checkbox';

export interface FilterState {
  search?: string;
  sort?: 'name' | 'createdAt' | 'updatedAt' | 'quotaUsage';
  sortOrder?: 'asc' | 'desc';
  filterActive?: boolean;
  filterInactive?: boolean;
  tier?: string;
}

interface ApplicationsFilterProps {
  filters: FilterState;
  onFilterChange: (filters: FilterState) => void;
  totalCount?: number;
}

export function ApplicationsFilter({
  filters,
  onFilterChange,
  totalCount,
}: ApplicationsFilterProps) {
  const [isOpen, setIsOpen] = useState(false);

  const activeFiltersCount = [
    filters.filterActive,
    filters.filterInactive,
    filters.tier,
  ].filter(Boolean).length;

  const updateFilter = <K extends keyof FilterState>(key: K, value: FilterState[K]) => {
    onFilterChange({ ...filters, [key]: value });
  };

  const clearFilters = () => {
    onFilterChange({
      search: '',
      sort: 'createdAt',
      sortOrder: 'desc',
    });
  };

  const hasFilters = filters.search ||
    filters.filterActive !== undefined ||
    filters.filterInactive !== undefined ||
    filters.tier;

  return (
    <div className="mb-6">
      {/* Search and Filter Bar */}
      <div className="flex gap-3 flex-wrap">
        {/* Search */}
        <div className="flex-1 min-w-[200px] relative">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-5 h-5 text-gray-400" />
          <Input
            placeholder="Search applications..."
            className="pl-10"
            value={filters.search || ''}
            onChange={(e) => updateFilter('search', e.target.value)}
          />
        </div>

        {/* Sort Dropdown */}
        <Select
          value={`${filters.sort || 'createdAt'}-${filters.sortOrder || 'desc'}`}
          onValueChange={(value) => {
            const [sort, sortOrder] = value.split('-') as [FilterState['sort'], FilterState['sortOrder']];
            onFilterChange({ ...filters, sort, sortOrder });
          }}
        >
          <SelectTrigger className="w-[180px]">
            <ArrowUpDown className="w-4 h-4 mr-2" />
            <SelectValue placeholder="Sort by..." />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="name-asc">Name (A-Z)</SelectItem>
            <SelectItem value="name-desc">Name (Z-A)</SelectItem>
            <SelectItem value="createdAt-desc">Newest First</SelectItem>
            <SelectItem value="createdAt-asc">Oldest First</SelectItem>
            <SelectItem value="updatedAt-desc">Recently Updated</SelectItem>
            <SelectItem value="quotaUsage-desc">Highest Usage</SelectItem>
            <SelectItem value="quotaUsage-asc">Lowest Usage</SelectItem>
          </SelectContent>
        </Select>

        {/* Filter Button */}
        <Popover open={isOpen} onOpenChange={setIsOpen}>
          <PopoverTrigger asChild>
            <Button variant="outline" className="relative">
              <SlidersHorizontal className="w-4 h-4 mr-2" />
              Filter
              {activeFiltersCount > 0 && (
                <Badge variant="destructive" className="absolute -top-2 -right-2 h-5 w-5 p-0 flex items-center justify-center text-xs">
                  {activeFiltersCount}
                </Badge>
              )}
            </Button>
          </PopoverTrigger>
          <PopoverContent className="w-80">
            <div className="space-y-4">
              <div className="flex items-center justify-between">
                <h4 className="font-semibold">Filters</h4>
                {hasFilters && (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={clearFilters}
                    className="h-auto p-1 text-xs"
                  >
                    <X className="w-4 h-4" />
                  </Button>
                )}
              </div>

              {/* Status Filter */}
              <div className="space-y-2">
                <label className="text-sm font-medium">Status</label>
                <div className="space-y-2">
                  <div className="flex items-center space-x-2">
                    <Checkbox
                      id="filter-active"
                      checked={filters.filterActive}
                      onCheckedChange={(checked) =>
                        updateFilter('filterActive', !!checked)
                      }
                    />
                    <label htmlFor="filter-active" className="text-sm">
                      Active
                    </label>
                  </div>
                  <div className="flex items-center space-x-2">
                    <Checkbox
                      id="filter-inactive"
                      checked={filters.filterInactive}
                      onCheckedChange={(checked) =>
                        updateFilter('filterInactive', !!checked)
                      }
                    />
                    <label htmlFor="filter-inactive" className="text-sm">
                      Inactive
                    </label>
                  </div>
                </div>
              </div>

              {/* Tier Filter */}
              <div className="space-y-2">
                <label className="text-sm font-medium">Tier</label>
                <Select
                  value={filters.tier || 'all'}
                  onValueChange={(value) =>
                    updateFilter('tier', value === 'all' ? undefined : value)
                  }
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="all">All Tiers</SelectItem>
                    <SelectItem value="free">Free</SelectItem>
                    <SelectItem value="pro">Pro</SelectItem>
                    <SelectItem value="enterprise">Enterprise</SelectItem>
                  </SelectContent>
                </Select>
              </div>

              <Button
                className="w-full"
                onClick={() => setIsOpen(false)}
              >
                Apply Filters
              </Button>
            </div>
          </PopoverContent>
        </Popover>

        {/* Clear Filters Button */}
        {hasFilters && (
          <Button variant="ghost" size="sm" onClick={clearFilters}>
            <X className="w-4 h-4 mr-2" />
            Clear
          </Button>
        )}
      </div>

      {/* Active Filters Display */}
      {hasFilters && (
        <div className="mt-3 flex flex-wrap gap-2">
          {filters.search && (
            <Badge variant="secondary" className="flex items-center gap-1">
              Search: &quot;{filters.search}&quot;
              <X
                className="w-3 h-3 cursor-pointer"
                onClick={() => updateFilter('search', '')}
              />
            </Badge>
          )}
          {filters.filterActive && (
            <Badge variant="secondary" className="flex items-center gap-1">
              Active
              <X
                className="w-3 h-3 cursor-pointer"
                onClick={() => updateFilter('filterActive', undefined)}
              />
            </Badge>
          )}
          {filters.filterInactive && (
            <Badge variant="secondary" className="flex items-center gap-1">
              Inactive
              <X
                className="w-3 h-3 cursor-pointer"
                onClick={() => updateFilter('filterInactive', undefined)}
              />
            </Badge>
          )}
          {filters.tier && (
            <Badge variant="secondary" className="flex items-center gap-1">
              {filters.tier.charAt(0).toUpperCase() + filters.tier.slice(1)}
              <X
                className="w-3 h-3 cursor-pointer"
                onClick={() => updateFilter('tier', undefined)}
              />
            </Badge>
          )}
        </div>
      )}

      {/* Results Count */}
      {totalCount !== undefined && (
        <p className="text-sm text-gray-600 dark:text-gray-400 mt-2">
          Showing {totalCount} application{totalCount !== 1 ? 's' : ''}
        </p>
      )}
    </div>
  );
}
