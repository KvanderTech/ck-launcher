import { useEffect, useState } from "react";

import backgroundOne from "../assets/bg-1.jpg";
import backgroundTwo from "../assets/bg-2.jpg";
import backgroundThree from "../assets/bg-3.jpg";

const backgrounds = [backgroundOne, backgroundTwo, backgroundThree];

export function BackgroundCarousel() {
  const [activeIndex, setActiveIndex] = useState(0);

  useEffect(() => {
    if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) return;
    const timer = window.setInterval(() => setActiveIndex((current) => (current + 1) % backgrounds.length), 14_000);
    return () => window.clearInterval(timer);
  }, []);

  return <div aria-hidden="true" className="background-carousel">
    {backgrounds.map((background, index) => <img className={index === activeIndex ? "is-active" : ""} key={background} src={background} />)}
  </div>;
}
