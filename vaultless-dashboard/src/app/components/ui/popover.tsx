"use client";

import * as React from "react";
import {
  useFloating,
  autoUpdate,
  offset,
  flip,
  shift,
  useClick,
  useDismiss,
  useRole,
  useInteractions,
  useId,
  FloatingPortal,
  FloatingFocusManager,
  type Placement,
} from "@floating-ui/react";
import { cn } from "./utils";

interface PopoverOptions {
  initialOpen?: boolean;
  placement?: Placement;
  modal?: boolean;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}

function usePopover({
  initialOpen = false,
  placement = "bottom",
  modal = true,
  open: controlledOpen,
  onOpenChange: setControlledOpen,
}: PopoverOptions = {}) {
  const [uncontrolledOpen, setUncontrolledOpen] = React.useState(initialOpen);
  const [labelId, setLabelId] = React.useState<string | undefined>();
  const [descriptionId, setDescriptionId] = React.useState<string | undefined>();

  const open = controlledOpen ?? uncontrolledOpen;
  const setOpen = setControlledOpen ?? setUncontrolledOpen;

  const data = useFloating({
    placement,
    open,
    onOpenChange: setOpen,
    whileElementsMounted: autoUpdate,
    middleware: [
      offset(4),
      flip({
        crossAxis: placement.includes("-"),
        fallbackAxisSideDirection: "end",
        padding: 5,
      }),
      shift({ padding: 5 }),
    ],
  });

  const context = data.context;

  const click = useClick(context, {
    enabled: controlledOpen === undefined,
  });
  const dismiss = useDismiss(context);
  const role = useRole(context);

  const interactions = useInteractions([click, dismiss, role]);

  return React.useMemo(
    () => ({
      open,
      setOpen,
      ...interactions,
      ...data,
      modal,
      labelId,
      descriptionId,
      setLabelId,
      setDescriptionId,
    }),
    [open, setOpen, interactions, data, modal, labelId, descriptionId]
  );
}

type ContextType = ReturnType<typeof usePopover> | null;

const PopoverContext = React.createContext<ContextType>(null);

function usePopoverContext() {
  const context = React.useContext(PopoverContext);

  if (context == null) {
    throw new Error("Popover components must be wrapped in <Popover />");
  }

  return context;
}

export function Popover({
  children,
  modal = false,
  ...restOptions
}: {
  children: React.ReactNode;
} & PopoverOptions) {
  // This can accept each prop as a separate prop, or accept a config object.
  const popover = usePopover({ modal, ...restOptions });
  return (
    <PopoverContext.Provider value={popover}>
      {children}
    </PopoverContext.Provider>
  );
}

interface PopoverTriggerProps {
  children: React.ReactNode;
  asChild?: boolean;
}

export const PopoverTrigger = React.forwardRef<
  HTMLElement,
  React.HTMLProps<HTMLElement> & PopoverTriggerProps
>(function PopoverTrigger({ children, asChild = false, ...props }, propRef) {
  const context = usePopoverContext();
  const childrenRef = (children as any).ref;
  const ref = React.useMemo(
    () => (node: HTMLElement | null) => {
      // Set the internal ref
      context.refs.setReference(node);
      // Set the forwarded ref
      if (typeof propRef === "function") {
        propRef(node);
      } else if (propRef) {
        propRef.current = node;
      }
      // Set the children ref if it exists
      if (typeof childrenRef === "function") {
        childrenRef(node);
      } else if (childrenRef) {
        childrenRef.current = node;
      }
    },
    [context.refs, propRef, childrenRef]
  );

  // `asChild` logic simplified for this boilerplate
  if (asChild && React.isValidElement(children)) {
    return React.cloneElement(
      children,
      context.getReferenceProps({
        ref,
        ...props,
        ...children.props,
        "data-state": context.open ? "open" : "closed",
      })
    );
  }

  return (
    <button
      ref={ref}
      type="button"
      // The user can style the trigger based on the state
      data-state={context.open ? "open" : "closed"}
      {...context.getReferenceProps(props)}
    >
      {children}
    </button>
  );
});

export const PopoverContent = React.forwardRef<
  HTMLDivElement,
  React.HTMLProps<HTMLDivElement>
>(function PopoverContent({ className, ...props }, propRef) {
  const { context: floatingContext, ...context } = usePopoverContext();
  const ref = React.useMemo(
    () => (node: HTMLDivElement | null) => {
      context.refs.setFloating(node);
      if (typeof propRef === "function") {
        propRef(node);
      } else if (propRef) {
        propRef.current = node;
      }
    },
    [context.refs, propRef]
  );

  if (!floatingContext.open) return null;

  return (
    <FloatingPortal>
      <FloatingFocusManager context={floatingContext} modal={context.modal}>
        <div
          ref={ref}
          style={{
            ...context.floatingStyles,
          }}
          aria-labelledby={context.labelId}
          aria-describedby={context.descriptionId}
          className={cn(
            "bg-popover text-popover-foreground z-50 w-72 rounded-md border p-4 shadow-md outline-hidden",
            "data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95",
            className
          )}
          data-state={context.open ? "open" : "closed"}
          {...context.getFloatingProps(props)}
        >
          {props.children}
        </div>
      </FloatingFocusManager>
    </FloatingPortal>
  );
});

export const PopoverAnchor = React.forwardRef<
  HTMLElement,
  React.HTMLProps<HTMLElement>
>(function PopoverAnchor({ children, ...props }, propRef) {
  const context = usePopoverContext();
  const ref = React.useMemo(
    () => (node: HTMLElement | null) => {
      context.refs.setReference(node);
      if (typeof propRef === "function") {
        propRef(node);
      } else if (propRef) {
        propRef.current = node;
      }
    },
    [context.refs, propRef]
  );

  return (
    <div ref={ref} {...props}>
      {children}
    </div>
  );
});
