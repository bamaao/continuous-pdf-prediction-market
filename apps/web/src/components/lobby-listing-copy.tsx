"use client";

import { listingHeadline } from "@cpm/sdk";
import { ListingLocale } from "./listing-locale";

/** Client island: translation toggle for lobby rows (server page cannot pass render props). */
export function LobbyListingCopy(props: {
  market: string;
  family?: number;
  title?: string | null;
  titleEn?: string | null;
  event?: string | null;
  eventEn?: string | null;
  description?: string | null;
  descriptionEn?: string | null;
  locale?: string | null;
  isTranslation?: boolean;
}) {
  return (
    <ListingLocale
      title={props.title}
      titleEn={props.titleEn}
      event={props.event}
      eventEn={props.eventEn}
      description={props.description}
      descriptionEn={props.descriptionEn}
      locale={props.locale}
      isTranslation={props.isTranslation}
    >
      {({ title, event, description }) => (
        <>
          <h2 className="font-display text-xl">{listingHeadline({ title, family: props.family, market: props.market })}</h2>
          {event ? <p className="mt-1 text-sm text-paper/80">{event}</p> : null}
          {description ? <p className="mt-1 line-clamp-3 text-sm leading-relaxed text-paper/60">{description}</p> : null}
        </>
      )}
    </ListingLocale>
  );
}
