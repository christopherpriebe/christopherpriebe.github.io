// Opens the email links written by _includes/email_link.liquid. The page
// carries the address's two halves reversed and apart, and they are joined
// only inside the click handler, so the address never sits in the source or
// in the live DOM (no mailto href, no visible text) for a scraper to read.

function reverse(text) {
  return Array.from(text || "").reverse().join("");
}

export function initEmailLinks() {
  document.querySelectorAll("[data-email-user][data-email-domain]").forEach((link) => {
    // Both halves come from _config.yml; with either missing there is nothing
    // to open, so the link is left inert rather than opening mailto:@.
    if (!link.getAttribute("data-email-user") || !link.getAttribute("data-email-domain")) {
      console.warn("Email link left inert: email_user or email_domain is not set in _config.yml.");
      link.removeAttribute("href");
      return;
    }

    link.addEventListener("click", (event) => {
      event.preventDefault();
      const user = reverse(link.getAttribute("data-email-user"));
      const domain = reverse(link.getAttribute("data-email-domain"));
      window.location.href = `mailto:${user}@${domain}`;
    });
  });
}
