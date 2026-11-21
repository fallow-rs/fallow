import Handlebars from "handlebars";

interface Profile {
  bio: string;
}

export const profileBio = (profile: Profile): Handlebars.SafeString =>
  new Handlebars.SafeString(profile.bio);
