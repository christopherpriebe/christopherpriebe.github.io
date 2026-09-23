// Reassembles the email links written by _includes/email_link.liquid. The page
// carries the address's two halves reversed and apart, so it never appears
// whole in the source; here they become a working mailto link.

function reverse(text) {
  return Array.from(text || "").reverse().join("");
}

export function initEmailLinks() {
  document.querySelectorAll("[data-email-user][data-email-domain]").forEach((link) => {
    // Both halves come from _config.yml; with either missing, a mailto:@ link
    // would be worse than the readable fallback already on the page.
    if (!link.getAttribute("data-email-user") || !link.getAttribute("data-email-domain")) {
      console.warn("Email link left as text: email_user or email_domain is not set in _config.yml.");
      return;
    }
    const address = `${reverse(link.getAttribute("data-email-user"))}@${reverse(link.getAttribute("data-email-domain"))}`;
    link.href = `mailto:${address}`;
    link.textContent = address;
  });
}
